+++
title = "Urx Documentation"
description = "Extracts URLs from OSINT Archives for Security Insights"
+++

<div class="landing-hero">
  <span class="badge">v0.8.0</span>
  <h1>The URL Extraction Tool<br>for Security Research</h1>
  <p>A fast, Rust-based CLI tool that extracts URLs from OSINT archives for bug bounty hunting, security research, and attack surface mapping.</p>
  <div class="landing-cta">
    <a href="/getting-started/installation/" class="btn-primary">Get Started</a>
    <a href="https://github.com/hahwul/urx" class="btn-secondary" target="_blank" rel="noopener">View on GitHub</a>
  </div>
</div>

<div class="landing-terminal">
  <div class="terminal-header">
    <span class="terminal-dot red"></span>
    <span class="terminal-dot yellow"></span>
    <span class="terminal-dot green"></span>
    <span class="terminal-title">Terminal</span>
  </div>
  <div class="terminal-body">
    <pre><code><span class="terminal-prompt">$</span> urx example.com --subs -e js,php --patterns api
<span class="terminal-output">https://example.com/api/v1/users.js
https://example.com/api/v2/config.php
https://sub.example.com/api/auth.js
https://dev.example.com/api/graphql.php
...</span>
<span class="terminal-prompt">$</span> urx example.com --providers wayback,cc,otx,vt -f json -o results.json
<span class="terminal-output">Fetching from 4 providers... done (1,247 URLs collected)</span></code></pre>
  </div>
</div>

<div class="landing-features">
  <div class="feature-card">
    <div class="feature-icon">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><line x1="2" y1="12" x2="22" y2="12"/><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"/></svg>
    </div>
    <h3>Multi-Source Collection</h3>
    <p>Fetch URLs from Wayback Machine, Common Crawl, OTX, VirusTotal, URLScan, and ZoomEye in parallel.</p>
  </div>
  <div class="feature-card">
    <div class="feature-icon">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/></svg>
    </div>
    <h3>Advanced Filtering</h3>
    <p>Filter by extensions, patterns, or presets. Control URL length, host parts, and more.</p>
  </div>
  <div class="feature-card">
    <div class="feature-icon">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><polyline points="14 2 14 8 20 8"/><line x1="16" y1="13" x2="8" y2="13"/><line x1="16" y1="17" x2="8" y2="17"/></svg>
    </div>
    <h3>Flexible Output</h3>
    <p>Plain text, JSON, and CSV formats. Stream to console, file, or pipe into other tools.</p>
  </div>
  <div class="feature-card">
    <div class="feature-icon">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M22 11.08V12a10 10 0 1 1-5.93-9.14"/><polyline points="22 4 12 14.01 9 11.01"/></svg>
    </div>
    <h3>URL Validation</h3>
    <p>Check HTTP status codes, extract additional links, and filter by response status.</p>
  </div>
  <div class="feature-card">
    <div class="feature-icon">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 0 1-9 9m9-9a9 9 0 0 0-9-9m9 9H3m9 9a9 9 0 0 1-9-9m9 9c1.66 0 3-4.03 3-9s-1.34-9-3-9m0 18c-1.66 0-3-4.03-3-9s1.34-9 3-9"/></svg>
    </div>
    <h3>Caching & Incremental</h3>
    <p>Built-in SQLite and Redis caching. Incremental scanning returns only new URLs.</p>
  </div>
  <div class="feature-card">
    <div class="feature-icon">
      <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/></svg>
    </div>
    <h3>Built with Rust</h3>
    <p>Async processing, parallel requests, and minimal resource footprint for maximum speed.</p>
  </div>
</div>
