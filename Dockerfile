# syntax=docker/dockerfile:1

FROM eclipse-temurin:21-jdk-alpine AS build
WORKDIR /workspace

# Resolve dependencies in their own layer so `docker build` only re-downloads
# them when the Gradle files change, not on every source edit.
COPY gradlew settings.gradle.kts build.gradle.kts ./
COPY gradle gradle
COPY backend/build.gradle.kts backend/build.gradle.kts
RUN ./gradlew :backend:dependencies --no-daemon

COPY backend/src backend/src
RUN ./gradlew :backend:bootJar --no-daemon -x test

FROM eclipse-temurin:21-jre-alpine AS runtime
WORKDIR /app

RUN addgroup -S umfl && adduser -S umfl -G umfl
COPY --from=build /workspace/backend/build/libs/*.jar app.jar

# The Flyway migrations used to ride inside the jar as a classpath resource.
# They now live at the repo root so they survive `backend/` being deleted at the
# end of the Rust port, which means this image has to carry them as ordinary
# files and tell Flyway where they went -- `application.yml`'s default of
# `../db` is relative to Gradle's working directory, not to /app.
COPY db /app/db
ENV DB_MIGRATIONS_DIR=/app/db

RUN chown -R umfl:umfl app.jar /app/db
USER umfl

EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=5s --start-period=30s --retries=5 \
  CMD wget -qO- http://localhost:8080/actuator/health | grep -q '"status":"UP"' || exit 1

ENTRYPOINT ["java", "-XX:MaxRAMPercentage=60", "-XX:+UseSerialGC", "-jar", "app.jar"]
