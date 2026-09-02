# syntax=docker/dockerfile:1

# Bakes this directory's SQL into the stock Flyway CLI image, so applying the
# schema needs nothing else off the host it runs on. That matters because the
# Rust backend (see backend-rs/crates/umfl-server/src/config.rs) deliberately
# carries no migration runner of its own -- Flyway is still "the migration
# mechanism", it just isn't embedded in the server process any more the way
# Spring Boot's `spring-boot-starter-flyway` embeds it in the Kotlin jar. This
# image is that mechanism's new home: one-shot, run to completion, then the
# backend starts. `deploy/docker-compose.prod.yml`'s VPS keeps only itself and
# a hand-managed `.env`, never a checkout of this repo, so the SQL has to
# travel inside the image the same way the backend and scraper images already
# carry their own jar/binary/`node_modules` rather than reading source off the
# host at runtime.
#
# Build context is this directory (`docker build -f db/Dockerfile db/`) --
# `migration/` and `seed/` are the whole set of inputs, so there is no
# workspace or dependency manifest to resolve first the way the backend images
# need one.
#
# `-alpine`: this image does nothing but run the bundled `flyway` CLI against
# a network Postgres, so the full JRE-plus-glibc base buys nothing a Rust or
# JVM backend image would actually use.
FROM flyway/flyway:13.4.0-alpine

# Locations are supplied at run time via `FLYWAY_LOCATIONS` (see
# docker-compose.yml and deploy/docker-compose.prod.yml), not baked in here,
# for the same reason `SPRING_PROFILES_ACTIVE` decides `spring.flyway.locations`
# rather than the jar deciding for itself: whether `seed/` (demo/dev fixtures)
# is part of the migrate run is an environment's call, and this image is the
# same one either environment pulls. Copying both directories in
# unconditionally costs nothing -- an unreferenced location is never read.
COPY migration /flyway/sql/migration
COPY seed /flyway/sql/seed

CMD ["migrate"]
