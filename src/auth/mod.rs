mod models;
mod oauth;
mod session;
#[cfg(windows)]
mod wam;
mod xsts;

#[cfg(windows)]
pub use wam::request_wam_token;

pub use models::MicrosoftOAuthResponse;
pub use oauth::{DeviceCode, poll_device_token, refresh_token_grant, request_device_code};
pub use session::{clear_refresh_token, load_refresh_token, save_refresh_token};
pub use xsts::request_xbox_live_token;
