//! Volumes list, create form, delete and prune.

use leptos::prelude::*;
use leptos::task::spawn_local;
use pier_api_types::{CreateVolume, PruneReport, Volume};

use crate::api;
use crate::components::{Banner, BannerKind, ConfirmButton, Layout, TextField};

/// Lists volumes and lets the user create, delete and prune them.
#[component]
#[expect(
    clippy::too_many_lines,
    reason = "one page = one view! block; splitting hurts readability"
)]
pub fn VolumesPage() -> impl IntoView {
    let refresh = RwSignal::new(0_u32);
    let volumes = LocalResource::new(move || {
        refresh.track();
        async { api::get::<Vec<Volume>>("/api/volumes").await }
    });
    let error = RwSignal::new(None::<String>);
    let notice = RwSignal::new(None::<String>);
    let name = RwSignal::new(String::new());

    let create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            let body = CreateVolume {
                name: name.get_untracked().trim().to_string(),
                driver: None,
            };
            match api::post::<_, Volume>("/api/volumes", &body).await {
                Ok(_) => {
                    name.set(String::new());
                    refresh.update(|n| *n += 1);
                }
                Err(err) => error.set(Some(err.message)),
            }
        });
    };

    let remove = move |volume: String| {
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match api::delete(&format!("/api/volumes/{volume}")).await {
                Ok(()) => refresh.update(|n| *n += 1),
                Err(err) => error.set(Some(err.message)),
            }
        });
    };

    let prune = move |()| {
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match api::post_empty::<PruneReport>("/api/volumes/prune").await {
                Ok(report) => {
                    let removed = report.volumes_deleted.len();
                    let names = report.volumes_deleted.join(", ");
                    notice.set(Some(if removed == 0 {
                        "No unused volumes to remove.".to_string()
                    } else {
                        format!("Removed {removed} unused volume(s): {names}")
                    }));
                    refresh.update(|n| *n += 1);
                }
                Err(err) => error.set(Some(err.message)),
            }
        });
    };

    view! {
        <Layout>
            <h1>"Volumes"</h1>
            <Banner message=error kind=BannerKind::Error />
            <Banner message=notice kind=BannerKind::Ok />
            <table>
                <thead>
                    <tr>
                        <th>"Name"</th>
                        <th>"Driver"</th>
                        <th>"Mountpoint"</th>
                        <th></th>
                    </tr>
                </thead>
                <tbody>
                    {move || match volumes.get() {
                        None => view! { <tr><td colspan="4" class="muted">"Loading…"</td></tr> }.into_any(),
                        Some(Err(err)) => view! { <tr><td colspan="4">{err.message}</td></tr> }.into_any(),
                        Some(Ok(list)) => list
                            .into_iter()
                            .map(|v| {
                                let volume_name = v.name.clone();
                                let message = format!("Delete volume {}? Its data will be lost.", v.name);
                                view! {
                                    <tr>
                                        <td>{v.name}</td>
                                        <td>{v.driver}</td>
                                        <td><code>{v.mountpoint}</code></td>
                                        <td class="actions">
                                            <ConfirmButton
                                                label="Delete"
                                                message=message
                                                on_confirm=move |()| remove(volume_name.clone())
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

            <h2>"Create volume"</h2>
            <form class="card" on:submit=create>
                <TextField label="Name" value=name placeholder="my-data" />
                <button type="submit">"Create volume"</button>
            </form>

            <h2>"Prune"</h2>
            <p class="muted">"Removes every volume that no container uses."</p>
            <ConfirmButton
                label="Prune unused volumes"
                message="Delete all volumes not used by any container? This cannot be undone."
                on_confirm=prune
            />
        </Layout>
    }
}
