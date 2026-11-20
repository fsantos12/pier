//! Pier web UI entry point: mounts the Leptos app and defines the client-side routes.

mod api;
mod components;
mod pages;

use leptos::mount::mount_to_body;
use leptos::prelude::*;
use leptos_router::components::{Redirect, Route, Router, Routes};
use leptos_router::path;

use pages::{ContainersPage, LoginPage, NetworksPage, SetupPage, VolumesPage};

/// Root component: the router and all pages.
#[component]
fn App() -> impl IntoView {
    view! {
        <Router>
            <Routes fallback=|| view! { <p class="auth-page">"Page not found."</p> }>
                <Route path=path!("/") view=|| view! { <Redirect path="/containers" /> } />
                <Route path=path!("/login") view=LoginPage />
                <Route path=path!("/setup") view=SetupPage />
                <Route path=path!("/containers") view=ContainersPage />
                <Route path=path!("/networks") view=NetworksPage />
                <Route path=path!("/volumes") view=VolumesPage />
            </Routes>
        </Router>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}
