mod browser;
mod live;
mod models;
mod oauth;
mod pop;
mod session;
#[cfg(windows)]
mod wam;

#[cfg(windows)]
pub use wam::request_wam_token;

pub use live::{XboxAuthorization, authorize_xbox_live};
pub use models::MicrosoftOAuthResponse;
pub use oauth::{
    DeviceCode, interactive_sign_in, poll_device_token, refresh_token_grant, request_device_code,
};
pub use session::{clear_refresh_token, load_refresh_token, save_refresh_token};
