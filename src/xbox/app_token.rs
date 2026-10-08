use std::collections::HashMap;
use std::ffi::c_void;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE,
    PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_PROTECTION_FLAGS, PAGE_READONLY,
    PAGE_READWRITE, PAGE_WRITECOPY, VirtualQueryEx,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

use crate::error::Error;

const TOKEN_PREFIX: &[u8] = b"XBL3.0 x=";
const READ_LIMIT: usize = 10_000;
const CHUNK: usize = 1024 * 1024;

const APP_TOKEN_MISSING: &str = "Could not read the Xbox PC app authorization. Open and sign in to the Xbox app, then try again.";

pub fn read_xbox_app_authorization() -> Result<String, Error> {
    let started = std::time::Instant::now();
    let pids = xbox_app_pids()?;
    if pids.is_empty() {
        return Err(Error::Xbox(APP_TOKEN_MISSING.into()));
    }

    let mut tokens = Vec::new();
    let mut opened = 0u32;
    for pid in pids {
        let Ok(process) =
            (unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) })
        else {
            continue;
        };
        let process = OwnedHandle(process);
        opened += 1;
        tokens.extend(tokens_in_process(process.0));
    }

    let candidates = tokens.len();
    let selected = select_app_token(tokens);
    tracing::info!(
        processes = opened,
        candidates,
        found = selected.is_some(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "scanned xbox app for presence authorization"
    );
    if opened == 0 {
        return Err(Error::Xbox(
            "The Xbox app is running, but its authorization could not be read.".into(),
        ));
    }
    selected.ok_or_else(|| Error::Xbox(APP_TOKEN_MISSING.into()))
}

pub(crate) fn select_app_token(candidates: impl IntoIterator<Item = String>) -> Option<String> {
    let mut counts = HashMap::<String, usize>::new();
    for token in candidates {
        *counts.entry(token).or_default() += 1;
    }
    counts
        .into_iter()
        .filter(|(_, count)| *count > 3)
        .max_by_key(|(_, count)| *count)
        .map(|(token, _)| token)
}

pub(crate) fn normalize_token(text: &str) -> Option<String> {
    let rest = text.strip_prefix("XBL3.0 x=")?;
    let (hash, jwt) = rest.split_once(';')?;
    if hash.is_empty() || !hash.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    let jwt_len = jwt
        .bytes()
        .position(|byte| !is_token_byte(byte))
        .unwrap_or(jwt.len());
    let jwt = &jwt[..jwt_len];
    if jwt.len() < 32 || jwt.bytes().filter(|byte| *byte == b'.').count() < 2 {
        return None;
    }
    Some(format!("XBL3.0 x={hash};{jwt}"))
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'=')
}

fn xbox_app_pids() -> Result<Vec<u32>, Error> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|err| Error::Xbox(format!("could not list processes: {err}")))?;
    let snapshot = OwnedHandle(snapshot);
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

    let mut pids = Vec::new();
    let mut found = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    while found.is_ok() {
        if exe_name(&entry).eq_ignore_ascii_case("XboxPcApp.exe") {
            pids.push(entry.th32ProcessID);
        }
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        found = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
    Ok(pids)
}

fn exe_name(entry: &PROCESSENTRY32W) -> String {
    let len = entry
        .szExeFile
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(entry.szExeFile.len());
    String::from_utf16_lossy(&entry.szExeFile[..len])
}

fn tokens_in_process(process: HANDLE) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut address = 0usize;
    loop {
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let wrote = unsafe {
            VirtualQueryEx(
                process,
                Some(address as *const c_void),
                &mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if wrote == 0 {
            break;
        }
        let base = info.BaseAddress as usize;
        let size = info.RegionSize;
        if size == 0 {
            break;
        }
        if info.State.contains(MEM_COMMIT) && is_readable(info.Protect) {
            scan_region(process, base, size, &mut seen, &mut tokens);
        }
        let next = base.saturating_add(size);
        if next <= address {
            break;
        }
        address = next;
        if tokens.len() > 20_000 {
            break;
        }
    }
    tokens
}

fn is_readable(protect: PAGE_PROTECTION_FLAGS) -> bool {
    if protect.contains(PAGE_GUARD) || protect.contains(PAGE_NOACCESS) {
        return false;
    }
    protect.contains(PAGE_READONLY)
        || protect.contains(PAGE_READWRITE)
        || protect.contains(PAGE_WRITECOPY)
        || protect.contains(PAGE_EXECUTE_READ)
        || protect.contains(PAGE_EXECUTE_READWRITE)
        || protect.contains(PAGE_EXECUTE_WRITECOPY)
}

fn scan_region(
    process: HANDLE,
    base: usize,
    size: usize,
    seen: &mut std::collections::HashSet<usize>,
    tokens: &mut Vec<String>,
) {
    let overlap = TOKEN_PREFIX.len() - 1;
    let mut offset = 0usize;
    let mut tail = Vec::new();
    while offset < size && tokens.len() <= 20_000 {
        let want = (size - offset).min(CHUNK);
        let mut buf = vec![0u8; want];
        let mut read = 0usize;
        let result = unsafe {
            ReadProcessMemory(
                process,
                (base + offset) as *const c_void,
                buf.as_mut_ptr().cast(),
                want,
                Some(&mut read),
            )
        };
        if result.is_err() || read == 0 {
            tail.clear();
            offset = offset.saturating_add(want);
            continue;
        }
        buf.truncate(read);
        let tail_len = tail.len();
        let mut window = std::mem::take(&mut tail);
        window.extend_from_slice(&buf);
        let mut search_from = 0usize;
        while let Some(found) = window[search_from..]
            .windows(TOKEN_PREFIX.len())
            .position(|candidate| candidate == TOKEN_PREFIX)
        {
            let index = search_from + found;
            let absolute = base + offset.saturating_sub(tail_len) + index;
            if seen.insert(absolute)
                && let Some(token) = read_token(process, absolute, base + size)
            {
                tokens.push(token);
            }
            search_from = index + 1;
            if tokens.len() > 20_000 {
                break;
            }
        }
        tail = if window.len() > overlap {
            window[window.len() - overlap..].to_vec()
        } else {
            window
        };
        offset += read;
    }
}

fn read_token(process: HANDLE, address: usize, region_end: usize) -> Option<String> {
    if address >= region_end {
        return None;
    }
    let len = (region_end - address).min(READ_LIMIT);
    let mut buf = vec![0u8; len];
    let mut read = 0usize;
    let result = unsafe {
        ReadProcessMemory(
            process,
            address as *const c_void,
            buf.as_mut_ptr().cast(),
            len,
            Some(&mut read),
        )
    };
    if result.is_err() || read == 0 {
        return None;
    }
    buf.truncate(read);
    let end = buf.iter().position(|byte| *byte == 0).unwrap_or(buf.len());
    let text = std::str::from_utf8(&buf[..end]).ok()?;
    normalize_token(text)
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_token(suffix: &str) -> String {
        format!("XBL3.0 x=1234;aaaa.bbbb.{suffix}cccccccccccccccccccccccccccccccc")
    }

    #[test]
    fn normalize_token_keeps_a_compact_jwt_and_drops_trailing_junk() {
        let raw = format!("{} END", sample_token("sig"));
        assert_eq!(
            normalize_token(&raw).as_deref(),
            Some(sample_token("sig").as_str())
        );
    }

    #[test]
    fn normalize_token_rejects_a_prefix_without_a_jwt() {
        assert!(normalize_token("XBL3.0 x=1234;not-a-token").is_none());
        assert!(
            normalize_token("x:XBL3.0 x=1234;aaaa.bbbb.cccccccccccccccccccccccccccccccc").is_none()
        );
    }

    #[test]
    fn select_app_token_requires_the_same_value_more_than_three_times() {
        let token = sample_token("sig");
        let rare = vec![token.clone(), token.clone(), token.clone()];
        assert!(select_app_token(rare).is_none());

        let mut repeated = vec![sample_token("other"); 4];
        repeated.extend(std::iter::repeat_n(token.clone(), 5));
        assert_eq!(select_app_token(repeated).as_deref(), Some(token.as_str()));
    }
}
