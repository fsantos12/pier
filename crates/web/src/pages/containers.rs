//! Containers list with lifecycle actions, create form and image pull.

use leptos::prelude::*;
use leptos::task::spawn_local;
use pier_api_types::{Container, ContainerDetail, CreateContainer, PullImage, PullResult};

use super::non_empty;
use crate::api;
use crate::components::{Banner, BannerKind, ConfirmButton, Layout, TextAreaField, TextField};

/// One argument per line, so no shell quoting is ever involved.
fn parse_cmd(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Lists containers and lets the user create, start, stop, restart and remove them.
#[component]
#[expect(
    clippy::too_many_lines,
    reason = "one page = one view! block; splitting hurts readability"
)]
pub fn ContainersPage() -> impl IntoView {
    let refresh = RwSignal::new(0_u32);
    let containers = LocalResource::new(move || {
        refresh.track();
        async { api::get::<Vec<Container>>("/api/containers").await }
    });
    let error = RwSignal::new(None::<String>);
    let notice = RwSignal::new(None::<String>);

    let image = RwSignal::new(String::new());
    let name = RwSignal::new(String::new());
    let cmd = RwSignal::new(String::new());
    let pull_ref = RwSignal::new(String::new());
    let pulling = RwSignal::new(false);

    let bump = move || refresh.update(|n| *n += 1);

    let create = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            let body = CreateContainer {
                image: image.get_untracked().trim().to_string(),
                name: non_empty(&name.get_untracked()),
                cmd: parse_cmd(&cmd.get_untracked()),
            };
            match api::post::<_, ContainerDetail>("/api/containers", &body).await {
                Ok(_) => {
                    name.set(String::new());
                    cmd.set(String::new());
                    bump();
                }
                Err(err) => error.set(Some(err.message)),
            }
        });
    };

    let pull = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        error.set(None);
        notice.set(None);
        pulling.set(true);
        spawn_local(async move {
            let body = PullImage {
                image: pull_ref.get_untracked().trim().to_string(),
            };
            match api::post::<_, PullResult>("/api/images/pull", &body).await {
                Ok(result) => notice.set(Some(format!("{}: {}", result.image, result.status))),
                Err(err) => error.set(Some(err.message)),
            }
            pulling.set(false);
        });
    };

    let act = move |id: String, verb: &'static str| {
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match api::action(&format!("/api/containers/{id}/{verb}")).await {
                Ok(()) => bump(),
                Err(err) => error.set(Some(err.message)),
            }
        });
    };

    let remove = move |id: String| {
        error.set(None);
        notice.set(None);
        spawn_local(async move {
            match api::delete(&format!("/api/containers/{id}")).await {
                Ok(()) => bump(),
                Err(err) => error.set(Some(err.message)),
            }
        });
    };

    view! {
        <Layout>
            <h1>"Containers"</h1>
            <Banner message=error kind=BannerKind::Error />
            <Banner message=notice kind=BannerKind::Ok />
            <table>
                <thead>
                    <tr>
                        <th>"Name"</th>
                        <th>"Image"</th>
                        <th>"State"</th>
                        <th>"Status"</th>
                        <th></th>
                    </tr>
                </thead>
                <tbody>
                    {move || match containers.get() {
                        None => view! { <tr><td colspan="5" class="muted">"Loading…"</td></tr> }.into_any(),
                        Some(Err(err)) => view! { <tr><td colspan="5">{err.message}</td></tr> }.into_any(),
                        Some(Ok(list)) => list
                            .into_iter()
                            .map(|c| {
                                let badge = format!("badge {}", c.state);
                                // Each button closure owns its own copy of the id.
                                let (start_id, stop_id, restart_id) = (c.id.clone(), c.id.clone(), c.id.clone());
                                let remove_id = c.id;
                                let message = format!("Remove container {}?", c.name);
                                view! {
                                    <tr>
                                        <td>{c.name}</td>
                                        <td>{c.image}</td>
                                        <td><span class=badge>{c.state}</span></td>
                                        <td class="muted">{c.status}</td>
                                        <td class="actions">
                                            <button on:click=move |_| act(start_id.clone(), "start")>"Start"</button>
                                            <button class="secondary" on:click=move |_| act(stop_id.clone(), "stop")>"Stop"</button>
                                            <button class="secondary" on:click=move |_| act(restart_id.clone(), "restart")>"Restart"</button>
                                            <ConfirmButton
                                                label="Remove"
                                                message=message
                                                on_confirm=move |()| remove(remove_id.clone())
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

            <h2>"Create container"</h2>
            <form class="card" on:submit=create>
                <TextField label="Image" value=image placeholder="alpine:3.20" />
                <TextField label="Name (optional)" value=name placeholder="my-container" />
                <TextAreaField label="Command (optional, one argument per line)" value=cmd placeholder="sleep&#10;3600" />
                <button type="submit">"Create container"</button>
            </form>

            <h2>"Pull image"</h2>
            <form class="inline" on:submit=pull>
                <TextField label="Image" value=pull_ref placeholder="nginx:1.27" />
                <button type="submit" disabled=move || pulling.get()>
                    {move || if pulling.get() { "Pulling…" } else { "Pull" }}
                </button>
            </form>
        </Layout>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_lines_become_an_argument_array() {
        assert_eq!(
            parse_cmd("sh\n-c\n echo hi; ls \n\n"),
            vec!["sh", "-c", "echo hi; ls"]
        );
        assert_eq!(parse_cmd("  \n "), Vec::<String>::new());
    }
}
