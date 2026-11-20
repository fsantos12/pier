//! Networks list, create form and delete.

use leptos::prelude::*;
use leptos::task::spawn_local;
use pier_api_types::{CreateNetwork, Network};

use super::non_empty;
use crate::api;
use crate::components::{
    Banner, BannerKind, CheckboxField, ConfirmButton, Layout, SelectField, TextField,
};

const DRIVERS: &[&str] = &["bridge", "overlay", "macvlan", "ipvlan"];

/// Lists networks and lets the user create and delete them.
#[component]
pub fn NetworksPage() -> impl IntoView {
    let refresh = RwSignal::new(0_u32);
    let networks = LocalResource::new(move || {
        refresh.track();
        async { api::get::<Vec<Network>>("/api/networks").await }
    });
    let error = RwSignal::new(None::<String>);

    let name = RwSignal::new(String::new());
    let driver = RwSignal::new("bridge".to_string());
    let subnet = RwSignal::new(String::new());
    let gateway = RwSignal::new(String::new());
    let internal = RwSignal::new(false);

    let create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        error.set(None);
        spawn_local(async move {
            let body = CreateNetwork {
                name: name.get_untracked().trim().to_string(),
                driver: Some(driver.get_untracked()),
                internal: internal.get_untracked(),
                subnet: non_empty(&subnet.get_untracked()),
                gateway: non_empty(&gateway.get_untracked()),
            };
            match api::post::<_, Network>("/api/networks", &body).await {
                Ok(_) => {
                    name.set(String::new());
                    refresh.update(|n| *n += 1);
                }
                Err(err) => error.set(Some(err.message)),
            }
        });
    };

    let remove = move |id: String| {
        error.set(None);
        spawn_local(async move {
            match api::delete(&format!("/api/networks/{id}")).await {
                Ok(()) => refresh.update(|n| *n += 1),
                Err(err) => error.set(Some(err.message)),
            }
        });
    };

    view! {
        <Layout>
            <h1>"Networks"</h1>
            <Banner message=error kind=BannerKind::Error />
            <table>
                <thead>
                    <tr>
                        <th>"Name"</th>
                        <th>"Driver"</th>
                        <th>"Scope"</th>
                        <th>"Subnet"</th>
                        <th>"ID"</th>
                        <th></th>
                    </tr>
                </thead>
                <tbody>
                    {move || match networks.get() {
                        None => view! { <tr><td colspan="6" class="muted">"Loading…"</td></tr> }.into_any(),
                        Some(Err(err)) => view! { <tr><td colspan="6">{err.message}</td></tr> }.into_any(),
                        Some(Ok(list)) => list
                            .into_iter()
                            .map(|n| {
                                let subnets = n.ipam.iter().filter_map(|p| p.subnet.clone()).collect::<Vec<_>>().join(", ");
                                let short_id: String = n.id.chars().take(12).collect();
                                let id = n.id.clone();
                                let message = format!("Delete network {}?", n.name);
                                view! {
                                    <tr>
                                        <td>{n.name}</td>
                                        <td>{n.driver}</td>
                                        <td>{n.scope}</td>
                                        <td>{subnets}</td>
                                        <td><code>{short_id}</code></td>
                                        <td class="actions">
                                            <ConfirmButton
                                                label="Delete"
                                                message=message
                                                on_confirm=move |()| remove(id.clone())
                                            />
                                        </td>
                                    </tr>
                                }
                            })
                            .collect_view()
                            .into_any(),
                    }}
                </tbody>
            </table>

            <h2>"Create network"</h2>
            <form class="card" on:submit=create>
                <TextField label="Name" value=name placeholder="my-network" />
                <SelectField label="Driver" value=driver options=DRIVERS />
                <TextField label="Subnet (optional)" value=subnet placeholder="172.28.0.0/16" />
                <TextField label="Gateway (optional)" value=gateway placeholder="172.28.0.1" />
                <CheckboxField label="Internal (no external access)" value=internal />
                <button type="submit">"Create network"</button>
            </form>
        </Layout>
    }
}
