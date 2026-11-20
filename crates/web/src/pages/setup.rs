//! First-run setup page: creates the admin account.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::NavigateOptions;
use leptos_router::hooks::use_navigate;
use pier_api_types::{Credentials, UserInfo};

use crate::api;
use crate::components::{Banner, BannerKind, TextField};

/// The "create the admin account" form.
#[component]
pub fn SetupPage() -> impl IntoView {
    let username = RwSignal::new(String::new());
    let password = RwSignal::new(String::new());
    let confirm = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let navigate = use_navigate();

    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        error.set(None);
        if password.get_untracked() != confirm.get_untracked() {
            error.set(Some("passwords do not match".into()));
            return;
        }
        busy.set(true);
        let navigate = navigate.clone();
        spawn_local(async move {
            let body = Credentials {
                username: username.get_untracked(),
                password: password.get_untracked(),
            };
            match api::post::<_, UserInfo>("/api/setup", &body).await {
                Ok(_) => navigate("/containers", NavigateOptions::default()),
                Err(err) => error.set(Some(err.message)),
            }
            busy.set(false);
        });
    };

    view! {
        <div class="auth-page">
            <h1>"Welcome to Pier"</h1>
            <p class="muted">"Create the administrator account (password: at least 8 characters)."</p>
            <Banner message=error kind=BannerKind::Error />
            <form class="card" on:submit=submit>
                <TextField label="Username" value=username />
                <TextField label="Password" value=password input_type="password" />
                <TextField label="Repeat password" value=confirm input_type="password" />
                <button type="submit" disabled=move || busy.get()>"Create account"</button>
            </form>
        </div>
    }
}
