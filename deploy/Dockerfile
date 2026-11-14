# Harrow dedicated server as a container image: an alternative to the
# systemd install (deploy/install.sh). Build from the repository root:
#
#   docker build -f deploy/Dockerfile \
#       --build-arg HARROW_BUILD_COMMIT=$(git rev-parse --short=10 HEAD) \
#       -t harrow-server .
#   docker run -d --name harrow --restart unless-stopped -p 5888:5888/udp harrow-server
#   docker logs -f harrow
#
# Own settings:   -v /srv/harrow/server.ron:/etc/harrow-server/server.ron:ro
# Shared secret:  -e HARROW_KEY='a long random secret'
# Another port:   -p 6000:6000/udp ... harrow-server --server 6000 --config /etc/harrow-server/server.ron
# (podman works the same: replace `docker` with `podman`.)
#
# HARROW_BUILD_COMMIT is the commit players must run (the build id check);
# without it the image reports build "unknown" and nobody can join.

# ---- Build: Rust on Debian 12 -------------------------------------------------------
FROM docker.io/library/rust:1.97.1-slim-bookworm AS build
ARG HARROW_BUILD_COMMIT=unknown
ARG JOBS=6
ENV HARROW_BUILD_COMMIT=${HARROW_BUILD_COMMIT} CARGO_BUILD_JOBS=${JOBS}
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
# The tuning file is also compiled in (the fallback when it can't be read).
COPY assets/config ./assets/config
# The lean server build: no window system, audio or gamepad libraries.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --no-default-features --features dedicated \
    && cp target/release/harrow /harrow \
    && strip /harrow

# ---- Run: the same Debian, just the server and the assets it loads ---------------------
FROM docker.io/library/debian:bookworm-slim
RUN useradd --system --no-create-home --home-dir /nonexistent --shell /usr/sbin/nologin harrow
WORKDIR /opt/harrow-server
COPY --from=build /harrow ./harrow
# Models (kit and props: the map's collision; characters' skeletons) and
# tuning. No textures: a dedicated server never loads real images.
COPY assets/models ./assets/models
COPY assets/config/tuning.ron ./assets/config/tuning.ron
COPY assets/config/server.ron /etc/harrow-server/server.ron
ENV BEVY_ASSET_ROOT=/opt/harrow-server
USER harrow
EXPOSE 5888/udp
STOPSIGNAL SIGTERM
ENTRYPOINT ["/opt/harrow-server/harrow"]
CMD ["--server", "--config", "/etc/harrow-server/server.ron"]
