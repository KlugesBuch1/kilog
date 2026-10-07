use crate::auth::MicrosoftOAuthResponse;
use crate::auth::oauth::DEFAULT_CLIENT_ID;
use crate::error::Error;

const AUTHORITY: &str = "https://login.microsoftonline.com";
const TENANT: &str = "consumers";
const SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";
const FALLBACK_CLIENT_ID: &str = "000000004c12ae6f";

pub async fn request_wam_token(hwnd: isize) -> Result<MicrosoftOAuthResponse, Error> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("kilog-wam".into())
        .spawn(move || {
            let _ = tx.send(request_on_sta(hwnd));
        })
        .map_err(|err| Error::OAuth(format!("could not start Web Account Manager: {err}")))?;
    rx.await
        .map_err(|_| Error::OAuth("Web Account Manager thread stopped".into()))?
}

fn request_on_sta(hwnd: isize) -> Result<MicrosoftOAuthResponse, Error> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};

    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }
        .ok()
        .map_err(|err| Error::OAuth(format!("could not start COM: {err}")))?;
    let result = request_token(hwnd);
    unsafe { CoUninitialize() };
    result
}

fn request_token(hwnd: isize) -> Result<MicrosoftOAuthResponse, Error> {
    use windows::Security::Authentication::Web::Core::{
        WebAuthenticationCoreManager, WebTokenRequest, WebTokenRequestPromptType,
        WebTokenRequestStatus,
    };
    use windows::core::HSTRING;

    let provider = match WebAuthenticationCoreManager::FindAccountProviderWithAuthorityAsync(
        &HSTRING::from(AUTHORITY),
        &HSTRING::from(TENANT),
    )
    .map_err(win_err)?
    .join()
    {
        Ok(provider) => provider,
        Err(err) if err.code().0 == 0 => WebAuthenticationCoreManager::FindAccountProviderAsync(
            &HSTRING::from("https://login.microsoft.com"),
        )
        .map_err(win_err)?
        .join()
        .map_err(win_err)?,
        Err(err) => return Err(win_err(err)),
    };

    let scope = HSTRING::from(SCOPE);
    for client_id in [DEFAULT_CLIENT_ID, FALLBACK_CLIENT_ID] {
        let request = WebTokenRequest::CreateWithPromptType(
            &provider,
            &scope,
            &HSTRING::from(client_id),
            WebTokenRequestPromptType::Default,
        )
        .map_err(win_err)?;

        let account = signed_in_account(&provider, client_id)?;
        let silent = if let Some(account) = account {
            WebAuthenticationCoreManager::GetTokenSilentlyWithWebAccountAsync(&request, &account)
        } else {
            WebAuthenticationCoreManager::GetTokenSilentlyAsync(&request)
        }
        .map_err(win_err)?
        .join()
        .map_err(win_err)?;
        let silent_status = silent.ResponseStatus().map_err(win_err)?;
        if let Some(token) = first_token(&silent).map_err(win_err)? {
            return Ok(session(token, client_id));
        }

        if silent_status != WebTokenRequestStatus::UserInteractionRequired
            && silent_status != WebTokenRequestStatus::AccountSwitch
        {
            continue;
        }

        let interactive = request_for_window(&request, hwnd)?;
        if let Some(token) = first_token(&interactive).map_err(win_err)? {
            return Ok(session(token, client_id));
        }
        if interactive.ResponseStatus().map_err(win_err)? == WebTokenRequestStatus::UserCancel {
            return Err(Error::OAuth("Windows sign-in was cancelled".into()));
        }
    }

    Err(Error::OAuth(
        "Windows has no Microsoft account for this app".into(),
    ))
}

fn signed_in_account(
    provider: &windows::Security::Credentials::WebAccountProvider,
    client_id: &str,
) -> Result<Option<windows::Security::Credentials::WebAccount>, Error> {
    use windows::Security::Authentication::Web::Core::WebAuthenticationCoreManager;
    use windows::core::HSTRING;

    let found = WebAuthenticationCoreManager::FindAllAccountsWithClientIdAsync(
        provider,
        &HSTRING::from(client_id),
    )
    .map_err(win_err)?
    .join()
    .map_err(win_err)?;
    let accounts = found.Accounts().map_err(win_err)?;
    if accounts.Size().map_err(win_err)? == 0 {
        return Ok(None);
    }
    let account = accounts.GetAt(0).map_err(win_err)?;
    let id = account.Id().map_err(win_err)?;
    let account = WebAuthenticationCoreManager::FindAccountAsync(provider, &id)
        .map_err(win_err)?
        .join()
        .map_err(win_err)?;
    Ok(Some(account))
}

fn request_for_window(
    request: &windows::Security::Authentication::Web::Core::WebTokenRequest,
    hwnd: isize,
) -> Result<windows::Security::Authentication::Web::Core::WebTokenRequestResult, Error> {
    use windows::Security::Authentication::Web::Core::{
        WebAuthenticationCoreManager, WebTokenRequestResult,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::WinRT::IWebAuthenticationCoreManagerInterop;
    use windows_future::IAsyncOperation;

    if hwnd == 0 {
        return WebAuthenticationCoreManager::RequestTokenAsync(request)
            .map_err(win_err)?
            .join()
            .map_err(win_err);
    }

    let interop: IWebAuthenticationCoreManagerInterop = windows::core::factory::<
        WebAuthenticationCoreManager,
        IWebAuthenticationCoreManagerInterop,
    >()
    .map_err(win_err)?;
    let operation = unsafe {
        interop.RequestTokenForWindowAsync::<_, IAsyncOperation<WebTokenRequestResult>>(
            HWND(hwnd as *mut core::ffi::c_void),
            request,
        )
    }
    .map_err(win_err)?;
    operation.join().map_err(win_err)
}

fn first_token(
    result: &windows::Security::Authentication::Web::Core::WebTokenRequestResult,
) -> windows::core::Result<Option<String>> {
    use windows::Security::Authentication::Web::Core::WebTokenRequestStatus;

    if result.ResponseStatus()? != WebTokenRequestStatus::Success {
        return Ok(None);
    }
    let responses = result.ResponseData()?;
    if responses.Size()? == 0 {
        return Ok(None);
    }
    let token = responses.GetAt(0)?.Token()?.to_string();
    if token.is_empty() {
        Ok(None)
    } else {
        Ok(Some(token))
    }
}

fn session(token: String, client_id: &str) -> MicrosoftOAuthResponse {
    MicrosoftOAuthResponse {
        access_token: token,
        refresh_token: None,
        client_id: client_id.to_owned(),
    }
}

fn win_err(err: windows::core::Error) -> Error {
    Error::OAuth(err.message())
}
