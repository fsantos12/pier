//! The routed pages.

mod containers;
mod login;
mod networks;
mod setup;
mod volumes;

pub use containers::ContainersPage;
pub use login::LoginPage;
pub use networks::NetworksPage;
pub use setup::SetupPage;
pub use volumes::VolumesPage;

/// Trims `value` and returns it, or `None` when nothing is left.
fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}
