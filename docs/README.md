# Jetpack Website

Cloudflare Pages builds and deploys this Hugo/Hextra site directly from GitHub.
GitHub Actions validates the Hugo site and checks generated reference docs; it
does not publish the website.

## Build configuration

| Setting | Value |
| --- | --- |
| Repository | `riffcc/jetpack` |
| Production branch | `main` |
| Framework preset | Hugo |
| Root directory | `docs` |
| Build command | `hugo --gc --minify --enableGitInfo` |
| Build output directory | `public` |
| Environment variable | `HUGO_VERSION=0.147.7` |
| Build watch include paths | `docs/*` |

Apply the pinned Hugo version to both production and preview environments.
The canonical domain is `jetpack.dragonfly.computer`. A Cloudflare redirect rule
for `jet.dragonfly.computer` points to the canonical domain, preserving the path
and query string. Keep that alias proxied so the redirect executes at the edge.

## Development

Use Hugo Extended 0.147.7 and Go 1.22 or later. Run `hugo server` from this directory.
Run `cargo run -- gen-reference` from the repository root after changing module or
CLI metadata, and commit the generated pages. CI checks them with
`cargo run --locked -- gen-reference --check`.

For rollback, the former DNS target for `jetpack.dragonfly.computer` was
`riffcc.github.io`. The previous GitHub Pages deployment remains available, but
the publishing workflow has been removed.
