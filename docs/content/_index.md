+++
template = "landing.html"

[extra.hero]
title = "Welcome to Urx!"
badge = "v0.8.0"
description = "Extracts URLs from OSINT Archives for Security Insights"
image = "/images/preview.jpg" # Background image
cta_buttons = [
    { text = "Get Started", url = "/getting_started/introduction", style = "primary" },
    { text = "View on GitHub", url = "https://github.com/hahwul/urx", style = "secondary" },
]

[extra.features_section]
title = "Essential Features"
description = "Discover urx's essential features for comprehensive attack surface detection and analysis."

[[extra.features]]
title = "Multi-Source URL Collection"
desc = "Fetch URLs from multiple OSINT archives in parallel, including Wayback Machine, Common Crawl, and OTX."
icon = "fa-solid fa-globe"

[[extra.features]]
title = "Advanced Filtering"
desc = "Filter results by file extensions, patterns, or presets (e.g., exclude images or resources)."
icon = "fa-solid fa-filter"

[[extra.features]]
title = "Flexible Output"
desc = "Supports plain text, JSON, and CSV formats. Output to console, file, or stream for pipeline integration."
icon = "fa-solid fa-file-export"

[[extra.features]]
title = "URL Testing & Validation"
desc = "Filter and validate URLs based on HTTP status codes, extract additional links, and perform live checks."
icon = "fa-solid fa-check-circle"

[[extra.features]]
title = "Performance & Concurrency"
desc = "Built with Rust for speed and efficiency, leveraging asynchronous processing and parallel requests."
icon = "fa-solid fa-bolt"

[[extra.features]]
title = "Customizable Network Options"
desc = "Configure proxies, timeouts, retries, parallelism, and more for robust network operations."
icon = "fa-solid fa-network-wired"

[extra.final_cta_section]
title = "Contributing"
description = "Urx is an open-source project made with ❤️. If you want to contribute to this project, please see CONTRIBUTING.md and submit a pull request with your cool content!"
button = { text = "View Contributing Guide", url = "https://github.com/hahwul/urx/blob/main/CONTRIBUTING.md" }
+++
