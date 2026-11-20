//! Login page. Redirects to first-run setup while no admin exists.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::hooks::use_navigate;
use pier_api_types::{Credentials, SetupStatus, UserInfo};

use crate::api;
use crate::components::{Banner, BannerKind, TextField};

/// The sign-in form.
#[component]
pub fn LoginPage() -> impl IntoView {
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let navigate = use_navigate();

    // Fresh installs have no user yet: send the visitor to the setup page.
    {
        let navigate = navigate.clone();
        spawn_local(async move {
            if let Ok(SetupStatus { needs_setup: true }) = api::get("/api/setup/status").await {
                navigate("/setup", NavigateOptions::default());
            }
        });
    }

    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        busy.set(true);
        error.set(None);
        let navigate = navigate.clone();
        spawn_local(async move {
            let body = Credentials {
                username: username.get_untracked(),
                password: password.get_untracked(),
            };
            match api::post::<_, UserInfo>("/api/auth/login", &body).await {
                Ok(_) => navigate("/containers", NavigateOptions::default()),
                Err(err) => error.set(Some(err.message)),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="auth-page">
            <h1>"Sign in to Pier"</h1>
            <Banner message=error kind=BannerKind::Error />
            <form class="card" on:submit=submit>
                <TextField label="Username" value=username />
                <TextField label="Password" value=password input_type="password" />
                <button type="submit" disabled=move || busy.get()>"Sign in"</button>
            </form>
        </div>
    }
}
