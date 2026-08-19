# syntax=docker/dockerfile:1

# Playwright's own image, pinned to the same version as the `playwright`
# dependency in package.json -- it ships Chromium plus the system libraries the
# browser needs, which is exactly what the backend's `eclipse-temurin:21-jre-alpine`
# runtime cannot provide. That mismatch is the reason this is a sidecar service
# rather than something bolted onto the backend image.
#
# Keep this tag and package.json's playwright version in step: the image's
# bundled browser must match the client driving it.
FROM mcr.microsoft.com/playwright:v1.62.1-noble

WORKDIR /app

# Dependencies in their own layer so a scraper edit doesn't re-resolve them.
COPY package.json package-lock.json ./
RUN npm ci --omit=dev

COPY lib.mjs scrape.mjs scrape-match.mjs server.mjs ./

# The image already runs as this user; being explicit keeps it from silently
# becoming root if the base image changes.
USER pwuser

# Reachable only on the compose network -- this port is deliberately not
# published to the host in either compose file.
ENV SCRAPER_HOST=0.0.0.0
ENV SCRAPER_PORT=3000
EXPOSE 3000

HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=5 \
  CMD node -e "fetch('http://127.0.0.1:3000/health').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"

CMD ["node", "server.mjs"]
