+++
title = "urx"
description = "Extract URLs from OSINT archives for security research. A fast, keyless Rust CLI."
template = "landing.html"
+++

<section class="hero">
  <div class="wrap">
    <h1>Extract every <span class="hot">URL</span> a domain ever exposed.</h1>
    <p class="hero-sub">A fast Rust CLI that queries nine OSINT archives in parallel, then filters and validates what comes back.</p>
    <div class="hero-cta">
      <a href="/getting-started/" class="btn btn-primary">
        Get started
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14M13 6l6 6-6 6"/></svg>
      </a>
      <a href="https://github.com/hahwul/urx" class="btn btn-ghost" target="_blank" rel="noopener">
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 1.5a10.5 10.5 0 0 0-3.32 20.46c.52.1.71-.23.71-.5v-1.75c-2.9.63-3.52-1.4-3.52-1.4-.47-1.2-1.16-1.52-1.16-1.52-.95-.65.07-.63.07-.63 1.05.07 1.6 1.08 1.6 1.08.93 1.6 2.45 1.14 3.05.87.09-.68.36-1.14.66-1.4-2.32-.26-4.75-1.16-4.75-5.16 0-1.14.4-2.07 1.07-2.8-.1-.26-.46-1.32.1-2.75 0 0 .88-.28 2.88 1.07a9.9 9.9 0 0 1 5.24 0c2-1.35 2.87-1.07 2.87-1.07.57 1.43.21 2.49.1 2.75.67.73 1.07 1.66 1.07 2.8 0 4.01-2.44 4.9-4.76 5.15.37.32.7.95.7 1.92v2.85c0 .28.19.61.72.5A10.5 10.5 0 0 0 12 1.5z"/></svg>
        View on GitHub
      </a>
    </div>
  </div>

  <div class="wrap showcase rv">
    <div class="term">
      <div class="term-bar"><span class="dot" aria-hidden="true"></span> urx &#183; recon</div>
      <div class="term-body"><pre><span class="prompt">$</span> urx dalfox.hahwul.com <span class="flag">--providers</span> wayback,otx <span class="flag">--check-status</span>

Domains         <span class="bar">[====================]</span> 1/1 processed
Wayback Machine <span class="bar">[====================]</span> <span class="done">done</span>
OTX             <span class="bar">[====================]</span> <span class="done">done</span>
Filtering       <span class="bar">[====================]</span> 199 URLs
Testing URLs    <span class="bar">[====================]</span> 199/199 complete

https://dalfox.hahwul.com/                         <span class="ok">[200 OK]</span>
https://dalfox.hahwul.com/.well-known/security.txt <span class="ok">[200 OK]</span>
https://dalfox.hahwul.com/llms.txt                 <span class="ok">[200 OK]</span>
https://dalfox.hahwul.com/sitemap.xml              <span class="ok">[200 OK]</span>
https://dalfox.hahwul.com/advanced/config/         <span class="gone">[404 Not Found]</span>
https://dalfox.hahwul.com/admin                    <span class="gone">[404 Not Found]</span>
<span class="muted">199 URLs &#183; 61 live &#183; 138 gone</span></pre></div>
    </div>
  </div>
</section>

<section class="section" id="providers">
  <div class="wrap">
    <div class="section-head rv">
      <h2 class="section-title">Nine sources, one command.</h2>
      <span class="contrail" aria-hidden="true"></span>
      <p class="section-sub">urx queries public web archives and threat-intel feeds at the same time, then merges and deduplicates the results. Five of them need no API key.</p>
    </div>
    <div class="providers rv">
      <div class="provider">
        <span class="p-name">Wayback Machine</span>
        <span class="p-flag">wayback</span>
        <span class="p-desc">The Internet Archive CDX index. Deep historical coverage, enabled by default.</span>
        <span class="p-key keyless">Keyless</span>
      </div>
      <div class="provider">
        <span class="p-name">Common Crawl</span>
        <span class="p-flag">cc</span>
        <span class="p-desc">The monthly Common Crawl URL index.</span>
        <span class="p-key keyless">Keyless</span>
      </div>
      <div class="provider">
        <span class="p-name">OTX</span>
        <span class="p-flag">otx</span>
        <span class="p-desc">AlienVault Open Threat Exchange passive DNS and URLs.</span>
        <span class="p-key keyless">Keyless</span>
      </div>
      <div class="provider">
        <span class="p-name">Arquivo.pt</span>
        <span class="p-flag">arquivo</span>
        <span class="p-desc">The Portuguese web archive CDX index.</span>
        <span class="p-key keyless">Keyless</span>
      </div>
      <div class="provider">
        <span class="p-name">Urlscan</span>
        <span class="p-flag">urlscan</span>
        <span class="p-desc">Urlscan.io search. Works anonymously; a key only raises the rate limit.</span>
        <span class="p-key keyless">Keyless</span>
      </div>
      <div class="provider">
        <span class="p-name">VirusTotal</span>
        <span class="p-flag">vt</span>
        <span class="p-desc">URLs VirusTotal has observed for the domain.</span>
        <span class="p-key">API key</span>
      </div>
      <div class="provider">
        <span class="p-name">ZoomEye</span>
        <span class="p-flag">zoomeye</span>
        <span class="p-desc">ZoomEye search results for the target.</span>
        <span class="p-key">API key</span>
      </div>
      <div class="provider">
        <span class="p-name">GitHub</span>
        <span class="p-flag">github</span>
        <span class="p-desc">GitHub Code Search, for URLs committed into public repositories.</span>
        <span class="p-key">API key</span>
      </div>
      <div class="provider">
        <span class="p-name">BeVigil</span>
        <span class="p-flag">bevigil</span>
        <span class="p-desc">Endpoints pulled out of unpacked Android apps.</span>
        <span class="p-key">API key</span>
      </div>
    </div>
    <p class="providers-note rv">Beyond the nine, urx also reads the target's own <code>robots.txt</code> and <code>sitemap.xml</code>, and will query any CDX server you point it at with <code>--cdx-endpoint</code>.</p>
  </div>
</section>

<section class="section" id="features">
  <div class="wrap">
    <div class="section-head rv">
      <h2 class="section-title">Built for the whole recon loop.</h2>
      <span class="contrail" aria-hidden="true"></span>
    </div>
    <div class="bento rv">
      <div class="cell cell-lead cell-accent">
        <span class="big-stat"><span>9</span> sources</span>
        <h3>Parallel collection</h3>
        <p>Async requests fan out to every enabled provider at once, then merge and deduplicate into a single URL set. Nothing waits on the slowest archive.</p>
      </div>
      <div class="cell">
        <span class="c-ico" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 5h18l-7 8v6l-4 2v-8z"/></svg></span>
        <h3>Advanced filtering</h3>
        <p>Filter by extension, pattern or preset. Control URL length and which host parts you keep.</p>
      </div>
      <div class="cell cell-code">
        <pre><span class="prompt">$</span> urx target.com \
    -e js,php \
    --patterns api,v1

<span class="muted">https://target.com/api/v1/auth.js
https://target.com/api/config.php
https://target.com/v1/users.js</span></pre>
      </div>
      <div class="cell">
        <span class="c-ico" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l7 3v6c0 4-3 6.5-7 9-4-2.5-7-5-7-9V6z"/><path d="M9 12l2 2 4-4"/></svg></span>
        <h3>URL validation</h3>
        <p>Check status codes, drop dead links, and pull more URLs out of the pages that answer.</p>
      </div>
      <div class="cell">
        <span class="c-ico" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3v5h5M14 3l5 5v11a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/><path d="M8 13h8M8 17h5"/></svg></span>
        <h3>Flexible output</h3>
        <p>Plain text, JSON, CSV or a ready-made wordlist, streamed to the console, a file, or the next tool in the pipe.</p>
      </div>
      <div class="cell cell-wide">
        <span class="c-ico" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v6c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 11v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6"/></svg></span>
        <h3>Caching and incremental scanning</h3>
        <p>A local SQLite or remote Redis cache skips domains you already covered, and incremental mode returns only what has appeared since the last run.</p>
      </div>
    </div>
  </div>
</section>

<section class="section">
  <div class="wrap">
    <div class="section-head rv">
      <h2 class="section-title">From a domain to a clean URL set.</h2>
      <span class="contrail" aria-hidden="true"></span>
      <p class="section-sub">One command runs the whole pipeline, and every stage maps to a flag you can tune.</p>
    </div>
    <div class="pipeline rv">
      <div class="stage">
        <span class="s-flag">--providers</span>
        <h3>Collect</h3>
        <p>Query every enabled archive in parallel and merge the results into one stream.</p>
      </div>
      <div class="stage">
        <span class="s-flag">-e &#183; --patterns</span>
        <h3>Filter</h3>
        <p>Keep only the extensions, patterns or presets you actually care about.</p>
      </div>
      <div class="stage">
        <span class="s-flag">--check-status</span>
        <h3>Validate</h3>
        <p>Probe what is live, record the status, and mine responses for more links.</p>
      </div>
      <div class="stage">
        <span class="s-flag">-f json &#183; -o</span>
        <h3>Output</h3>
        <p>Write text, JSON, CSV or a wordlist to a file, or pipe it straight into the next tool.</p>
      </div>
    </div>
  </div>
</section>

<section class="section">
  <div class="wrap">
    <div class="install rv">
      <h2>Install urx and start collecting.</h2>
      <p>Available on Cargo, Homebrew and as a container image. No account, no key to get going.</p>
      <div class="install-cmds">
        <div class="cmd"><span class="prompt">$</span> cargo install urx <span class="via">Cargo</span></div>
        <div class="cmd"><span class="prompt">$</span> brew install urx <span class="via">Homebrew</span></div>
        <div class="cmd"><span class="prompt">$</span> docker pull ghcr.io/hahwul/urx <span class="via">Docker</span></div>
      </div>
    </div>
  </div>
</section>
