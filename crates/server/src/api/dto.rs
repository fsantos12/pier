//! Conversions between `pier-core` domain types and `pier-api-types` wire types.
//!
//! This is the boundary the blueprint asks for: core never sees HTTP shapes and the
//! web crate never sees core. Both sides are foreign to this crate, so conversions are
//! plain functions rather than `From` impls.

use pier_api_types as api;
use pier_core::model as core;

pub(super) fn system_info(info: core::SystemInfo, endpoint: String) -> api::SystemInfo {
    api::SystemInfo {
        docker_version: info.docker_version,
        api_version: info.api_version,
        os: info.os,
        arch: info.arch,
        kernel_version: info.kernel_version,
        endpoint,
        pier_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

pub(super) fn network(n: core::Network) -> api::Network {
    api::Network {
        id: n.id,
        name: n.name,
        driver: n.driver,
        scope: n.scope,
        internal: n.internal,
        attachable: n.attachable,
        created: n.created,
        ipam: n
            .ipam
            .into_iter()
            .map(|p| api::IpamPool {
                subnet: p.subnet,
                gateway: p.gateway,
            })
            .collect(),
        labels: n.labels,
        containers: n
            .containers
            .into_iter()
            .map(|c| api::NetworkAttachment {
                container_id: c.container_id,
                name: c.name,
                ipv4_address: c.ipv4_address,
            })
            .collect(),
    }
}

pub(super) fn network_create(req: api::CreateNetwork) -> core::NetworkCreate {
    core::NetworkCreate {
        name: req.name,
        driver: req.driver,
        internal: req.internal,
        subnet: req.subnet,
        gateway: req.gateway,
    }
}

pub(super) fn volume(v: core::Volume) -> api::Volume {
    api::Volume {
        name: v.name,
        driver: v.driver,
        mountpoint: v.mountpoint,
        scope: v.scope,
        created_at: v.created_at,
        labels: v.labels,
    }
}

pub(super) fn volume_create(req: api::CreateVolume) -> core::VolumeCreate {
    core::VolumeCreate {
        name: req.name,
        driver: req.driver,
    }
}

pub(super) fn prune_report(r: core::PruneReport) -> api::PruneReport {
    api::PruneReport {
        volumes_deleted: r.volumes_deleted,
        space_reclaimed: r.space_reclaimed,
    }
}

pub(super) fn container(c: core::Container) -> api::Container {
    api::Container {
        id: c.id,
        name: c.name,
        image: c.image,
        state: c.state,
        status: c.status,
        created: c.created,
        ports: c
            .ports
            .into_iter()
            .map(|p| api::PortMapping {
                private_port: p.private_port,
                public_port: p.public_port,
                protocol: p.protocol,
                ip: p.ip,
            })
            .collect(),
        networks: c.networks,
        labels: c.labels,
    }
}

pub(super) fn container_detail(d: core::ContainerDetail) -> api::ContainerDetail {
    api::ContainerDetail {
        container: container(d.container),
        state_detail: api::ContainerState {
            status: d.state_detail.status,
            running: d.state_detail.running,
            exit_code: d.state_detail.exit_code,
            started_at: d.state_detail.started_at,
            finished_at: d.state_detail.finished_at,
        },
        entrypoint: d.entrypoint,
        cmd: d.cmd,
        restart_count: d.restart_count,
        mounts: d
            .mounts
            .into_iter()
            .map(|m| api::ContainerMount {
                kind: m.kind,
                name: m.name,
                source: m.source,
                destination: m.destination,
                read_write: m.read_write,
            })
            .collect(),
    }
}

pub(super) fn container_create(req: api::CreateContainer) -> core::ContainerCreate {
    core::ContainerCreate {
        image: req.image,
        name: req.name,
        cmd: req.cmd,
    }
}

pub(super) fn pull_result(r: core::PullReport) -> api::PullResult {
    api::PullResult {
        image: r.image,
        status: r.status,
    }
}
