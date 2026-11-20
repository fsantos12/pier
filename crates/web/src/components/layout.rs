//! The page frame: header with navigation, Docker host info and logout.

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_router::components::A;
use leptos_router::hooks::use_navigate;
use pier_api_types::{SystemInfo, UserInfo};

use crate::api;

/// Wraps a page for signed-in users. It verifies the session (a 401 redirects to the
/// login page) and renders the header around `children`.
#[component]
pub fn Layout(children: Children) -> impl IntoView {
    let user = LocalResource::new(|| async { api::get::<UserInfo>("/api/auth/me").await });
    let system = LocalResource::new(|| async { api::get::<SystemInfo>("/api/system").await });
    let navigate = use_navigate();

    let logout = move |_| {
        let navigate = navigate.clone();
        spawn_local(async move {
            // Whatever the server says, the user wants out: go to the login page.
            let _ = api::action("/api/auth/logout").await;
            navigate("/login", leptos_router::NavigateOptions::default());
        });
    };

    view! {
        <header class="top">
            <span class="brand">"Pier"</span>
            <nav>
                <A href="/containers">"Containers"</A>
                <A href="/networks">"Networks"</A>
                <A href="/volumes">"Volumes"</A>
            </nav>
            <span class="sysinfo">
                {move || {
                    system
                        .get()
                        .map(|result| match result {
                            Ok(info) => format!("Docker {} · {}/{}", info.docker_version, info.os, info.arch),
                            Err(err) => format!("Docker unavailable: {err}"),
                        })
                }}
            </span>
            <span class="muted">
                {move || user.get().and_then(Result::ok).map(|u| u.username)}
            </span>
            <button class="secondary" on:click=logout>"Log out"</button>
        </header>
        <main>{children()}</main>
    }
}
