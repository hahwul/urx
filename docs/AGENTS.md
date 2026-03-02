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
├── config.toml              # Site configuration
├── AGENTS.md                # This file
├── content/                 # Markdown content files
│   ├── index.md             # Landing page
│   ├── getting-started/     # Getting started section
│   │   ├── _index.md
│   │   ├── installation.md
│   │   └── quick-start.md
│   ├── usage/               # Usage guides
│   │   ├── _index.md
│   │   ├── cli-options.md
│   │   ├── configuration.md
│   │   └── examples.md
│   ├── advanced/            # Advanced topics
│   │   ├── _index.md
│   │   ├── integration.md
│   │   ├── caching.md
│   │   └── performance.md
│   ├── reference/           # Reference material
│   │   ├── _index.md
│   │   ├── environment-variables.md
│   │   └── changelog.md
│   └── community/           # Community & contributing
│       ├── _index.md
│       └── contributing.md
├── templates/               # Jinja2 templates
│   ├── header.html          # HTML head section
│   ├── footer.html          # Footer with active-link JS
│   ├── page.html            # Individual page template
│   ├── section.html         # Section listing template
│   ├── 404.html             # Not found page
│   ├── taxonomy.html
│   ├── taxonomy_term.html
│   └── shortcodes/
│       └── alert.html
└── static/                  # Static assets
    ├── CNAME                # DNS configuration
    ├── favicon.ico
    ├── css/
    │   └── style.css        # Main stylesheet (includes dark mode)
    └── images/
        ├── urx-dark.png     # Logo (used in header)
        ├── urx-light.png
        ├── urx.png
        ├── logo.png
        ├── preview.jpg      # OG image
        └── social.jpg
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

- **page.html** / **section.html** — Both include the full sidebar navigation and header
- **header.html** — `<head>` section with meta tags, favicon, CSS
- **footer.html** — Footer text, active-link highlighting JS, closing tags

## Styling

- Single CSS file: `static/css/style.css`
- CSS variables for theming (light/dark)
- Dark mode via `@media (prefers-color-scheme: dark)`
- Responsive: sidebar hidden on mobile with toggle button

## Notes for AI Agents

1. **Always preserve TOML front matter** when editing content files.
2. **Use `hwaro serve`** to preview changes locally.
3. **Check `config.toml`** for site-wide settings.
4. **Template Syntax:** Standard Jinja2 syntax.
5. **Keep URLs relative** using `{{ base_url }}` in templates, or absolute paths (`/getting-started/`) in markdown.
6. **Sidebar navigation** is duplicated in both `page.html` and `section.html` — update both when adding/removing pages.
7. **ZoomEye** is the most recently added provider — ensure it's included in provider lists and examples.
