# AGENTS.md - AI Agent Instructions for Urx Documentation Site

This document provides instructions for AI agents working on the Urx documentation website.

## Project Overview

This is the documentation site for [Urx](https://github.com/hahwul/urx), a fast Rust-based CLI tool for extracting URLs from OSINT archives. The site is built with [Hwaro](https://github.com/hahwul/hwaro), a static site generator written in Crystal.

## Site URL

- **Production**: https://urx.hahwul.com
- **Local dev**: http://localhost:3000

## Hwaro Usage

### Essential Commands

| Command | Description |
|---------|-------------|
| `hwaro build` | Build the site to `public/` directory |
| `hwaro serve` | Start development server with live reload |
| `hwaro serve -p 8080` | Serve on custom port |

### Build & Serve Options

- **Drafts:** `hwaro build --drafts` / `hwaro serve --drafts`
- **Port:** `hwaro serve -p 8080` (Default: 3000)
- **Open:** `hwaro serve --open` (Open browser automatically)
- **Base URL:** `hwaro build --base-url "https://urx.hahwul.com"`

## Directory Structure

```
docs/
├── config.toml              Site config, plugins, SEO, search, OG
├── content/
│   ├── index.md             Landing page (raw HTML, template = "landing.html")
│   ├── getting-started/     _index.md + installation, quick-start
│   ├── guide/               _index.md + cli-options, configuration, examples,
│   │                        environment-variables, caching, integration, performance
│   └── about/               _index.md + contributing
├── templates/
│   ├── header.html          <head>, favicon set, fonts, no-flash theme script
│   ├── footer.html          Footer plus the deferred script tags
│   ├── page.html            Leaf docs page
│   ├── section.html         Section index page
│   ├── landing.html         Home page shell (landing.css comes from header.html)
│   ├── 404.html
│   ├── partials/
│   │   ├── nav.html             Top nav: brand, links, search, theme, GitHub
│   │   ├── sidebar.html         Docs sidebar, DERIVED from site.sections
│   │   ├── document.html        Breadcrumb, heading, body, TOC rail
│   │   ├── page-navigation.html Prev/next within the section
│   │   └── search.html          Search modal markup
│   └── shortcodes/alert.html
├── static/
│   ├── css/style.css        Tokens, shell, prose, code, search, responsive
│   ├── css/landing.css      Landing-only sections
│   ├── js/                  theme, search, toc, docs, codecopy, home
│   ├── fonts/               Self-hosted Space Grotesk + JetBrains Mono (woff2)
│   ├── images/              mark, mark-light, lockups, og-card, preview
│   ├── icons/               favicon set + site.webmanifest
│   └── CNAME
├── tools/brand/generate.sh  Regenerates every logo, icon and OG asset
└── DESIGN.md                The design contract. Read it before restyling.
```

## Content Management

### Front Matter Format

All content uses **TOML** front matter (`+++` delimiters):

```toml
+++
title = "Page Title"
weight = 1
description = "Optional description for SEO"
+++

Markdown content here.
```

### Section Files (`_index.md`)

Each directory under `content/` has an `_index.md` that defines the section:

```toml
+++
title = "Section Title"
weight = 1
sort_by = "weight"
+++

Optional section description shown on the section page.
```

### Key Front Matter Fields

| Field       | Type    | Description                          |
|-------------|---------|--------------------------------------|
| title       | string  | Page title (required)                |
| weight      | integer | Sort order (lower = first)           |
| description | string  | Page description for SEO             |
| draft       | boolean | If true, excluded from production    |
| sort_by     | string  | Section sort: "weight", "date", "title" |

## Templates

Templates use Jinja2 syntax. Key variables:
- `{{ site.title }}` — Site title from config.toml
- `{{ page.title }}` — Current page title
- `{{ content }}` — Rendered markdown content
- `{{ base_url }}` — Site base URL
- `{{ page.section }}` — Section name
- `{{ section.list }}` — Section children listing

### Template Files

- **landing.html** — Full-width landing template (no docs sidebar). The home page (`content/index.md`) opts in via `template = "landing.html"` in its front matter. `landing.css` is linked from `header.html` when `page.section == ""` (the home page).
- **page.html** / **section.html** — Docs templates. Both pull in the shared nav and sidebar partials.
- **partials/nav.html** — Shared top navigation (brand, links, search, theme toggle, GitHub, mobile menu button).
- **partials/sidebar.html** — Shared docs sidebar, derived from `site.sections`; there is no hand-kept link list to edit.
- **header.html** — `<head>`: meta, self-hosted fonts (Space Grotesk / JetBrains Mono), no-flash theme script, CSS.
- **footer.html** — Footer plus the deferred script tags: `theme.js` and `search.js` everywhere, `home.js` on the landing page, `toc.js` / `docs.js` / `codecopy.js` on docs pages.

## Styling

- `static/css/style.css` — shared tokens, theme system, top nav, docs shell, prose, code, footer.
- `static/css/landing.css` — landing-only sections (hero, terminal, providers, bento, pipeline, install).
- Light/dark via CSS variables. Default follows `@media (prefers-color-scheme: dark)`; a header toggle overrides it and persists to `localStorage` (`urx-theme`), read by a no-flash inline script in `header.html`.
- One accent color: ignition orange (`--ignition: #ff5b29`). Code blocks stay dark in both themes by design (embedded-terminal look).
- Responsive: sidebar hidden on mobile with a toggle button.

## Notes for AI Agents

1. **Always preserve TOML front matter** when editing content files.
2. **Use `hwaro serve`** to preview changes locally, and `hwaro doctor` to check config and templates.
3. **Check `config.toml`** for site-wide settings.
4. **Template syntax** is Crinja (Jinja2 for Crystal). Note that `{{ ... }}` is not allowed inside a `{# ... #}` comment.
5. **Keep URLs relative** using `{{ base_url }}` in templates, or absolute paths (`/getting-started/`) in markdown.
6. **The sidebar and prev/next are derived** from `site.sections` and front-matter `weight`. Never hand-maintain a link list. Every new page needs `title`, `description`, `weight` and `toc = true`.
7. **Read `DESIGN.md` before changing anything visual.** It records the palette, the amber-on-light contrast rule, the radius system and the motion policy.
8. **Brand assets are generated, not hand-edited.** Run `tools/brand/generate.sh` to rebuild the mark, icons, lockups and OG card from `tools/brand/src/mark-source.jpg`.
9. **ZoomEye, GitHub and BeVigil** are the most recently added providers. The catalog in `src/app/catalog.rs` is the source of truth for provider lists: there are nine, five of them keyless.
