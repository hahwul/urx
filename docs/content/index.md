+++
title = "urx"
description = "Extract URLs from OSINT archives for security research. A fast, keyless Rust CLI."
template = "landing.html"
+++

<section class="hero">
  <div class="starfield" aria-hidden="true">
    <div class="stars"></div>
    <div class="grid-fade"></div>
  </div>
  <div class="wrap hero-grid">
    <div class="hero-copy">
      <span class="eyebrow">OSINT URL Discovery</span>
      <h1>Extract every <span class="hot">URL</span> a domain ever exposed.</h1>
      <p class="hero-sub">A fast Rust CLI that pulls URLs from OSINT archives in parallel, then filters and validates them for recon.</p>
      <div class="hero-cta">
        <a href="/getting-started/installation/" class="btn btn-primary">
          Get started
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12h14M13 6l6 6-6 6"/></svg>
        </a>
        <a href="https://github.com/hahwul/urx" class="btn btn-ghost" target="_blank" rel="noopener">
          <svg viewBox="0 0 24 24" fill="currentColor"><path d="M12 1.5a10.5 10.5 0 0 0-3.32 20.46c.52.1.71-.23.71-.5v-1.75c-2.9.63-3.52-1.4-3.52-1.4-.47-1.2-1.16-1.52-1.16-1.52-.95-.65.07-.63.07-.63 1.05.07 1.6 1.08 1.6 1.08.93 1.6 2.45 1.14 3.05.87.09-.68.36-1.14.66-1.4-2.32-.26-4.75-1.16-4.75-5.16 0-1.14.4-2.07 1.07-2.8-.1-.26-.46-1.32.1-2.75 0 0 .88-.28 2.88 1.07a9.9 9.9 0 0 1 5.24 0c2-1.35 2.87-1.07 2.87-1.07.57 1.43.21 2.49.1 2.75.67.73 1.07 1.66 1.07 2.8 0 4.01-2.44 4.9-4.76 5.15.37.32.7.95.7 1.92v2.85c0 .28.19.61.72.5A10.5 10.5 0 0 0 12 1.5z"/></svg>
          View on GitHub
        </a>
      </div>
      <div class="hero-meta">
        <div><span class="n">7</span><span class="l">Sources</span></div>
        <div><span class="n">5</span><span class="l">Keyless</span></div>
        <div><span class="n">3</span><span class="l">Output formats</span></div>
      </div>
    </div>
    <div class="rocket-stage" aria-hidden="true">
      <div class="orbit o2"></div>
      <div class="orbit o1"></div>
      <div class="rocket-glow"></div>
      <span class="thrust"></span>
      <img src="/images/urx.png" alt="urx rocket" class="hero-rocket">
    </div>
  </div>
</section>

<section class="section term-section">
  <div class="wrap reveal">
    <div class="term">
      <div class="term-bar">
        <span class="term-dot r"></span><span class="term-dot y"></span><span class="term-dot g"></span>
        <span class="term-title">urx · recon</span>
      </div>
      <div class="term-body"><pre><span class="prompt">$</span> urx dalfox.hahwul.com <span class="flag">--providers</span> wayback,otx,vt,urlscan <span class="flag">--check-status</span>

Domains         <span class="bar">[====================]</span> 1/1 processed
Wayback Machine <span class="bar">[====================]</span> <span class="done">done</span>
OTX             <span class="bar">[====================]</span> <span class="done">done</span>
VirusTotal      <span class="bar">[====================]</span> <span class="done">done</span>
Urlscan         <span class="bar">[====================]</span> <span class="done">done</span>
Filtering       <span class="bar">[====================]</span> 2408 URLs
Testing URLs    <span class="bar">[====================]</span> 2408/2408 complete

https://dalfox.hahwul.com/                         <span class="tag-ok">[200 OK]</span>
https://dalfox.hahwul.com/.well-known/security.txt <span class="tag-ok">[200 OK]</span>
https://dalfox.hahwul.com/docs/getting-started/    <span class="tag-ok">[200 OK]</span>
https://dalfox.hahwul.com/assets/js/main.min.js    <span class="tag-ok">[200 OK]</span>
https://dalfox.hahwul.com/allposts.html            <span class="tag-err">[404 Not Found]</span>
<span class="muted">... found 2408 URLs</span> <span class="cursor"></span></pre></div>
    </div>
  </div>
</section>

<section class="section" id="providers">
  <div class="wrap">
    <div class="section-head reveal">
      <h2 class="section-title">Seven archives, one command.</h2>
      <p class="section-sub">urx queries public web archives and threat-intel feeds at the same time. Five work with no API key at all.</p>
    </div>
    <div class="providers reveal">
      <div class="provider span-2">
        <span class="p-key keyless">Keyless</span>
        <span class="p-name">Wayback Machine</span>
        <span class="p-flag">--providers wayback</span>
        <span class="p-desc">The Internet Archive's CDX index. Deep historical coverage, on by default.</span>
      </div>
      <div class="provider">
        <span class="p-key keyless">Keyless</span>
        <span class="p-name">Common Crawl</span>
        <span class="p-flag">cc</span>
      </div>
      <div class="provider">
        <span class="p-key keyless">Keyless</span>
        <span class="p-name">OTX</span>
        <span class="p-flag">otx</span>
      </div>
      <div class="provider">
        <span class="p-key keyless">Keyless</span>
        <span class="p-name">Arquivo.pt</span>
        <span class="p-flag">arquivo</span>
      </div>
      <div class="provider">
        <span class="p-key">API key</span>
        <span class="p-name">VirusTotal</span>
        <span class="p-flag">vt</span>
      </div>
      <div class="provider">
        <span class="p-key keyless">Anonymous</span>
        <span class="p-name">URLScan</span>
        <span class="p-flag">urlscan</span>
      </div>
      <div class="provider">
        <span class="p-key">API key</span>
        <span class="p-name">ZoomEye</span>
        <span class="p-flag">zoomeye</span>
      </div>
    </div>
  </div>
</section>

<section class="section" id="features">
  <div class="wrap">
    <div class="section-head reveal">
      <h2 class="section-title">Built for the whole recon loop.</h2>
    </div>
    <div class="bento reveal">
      <div class="cell feature">
        <div class="big-num"><span>7</span> sources</div>
        <h3>Parallel collection</h3>
        <p>Async requests fan out to every enabled provider at once, then merge and deduplicate into a single URL set. No waiting on one slow archive.</p>
      </div>
      <div class="cell">
        <div class="c-ico"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 5h18l-7 8v6l-4 2v-8z"/></svg></div>
        <h3>Advanced filtering</h3>
        <p>Filter by extension, pattern, or presets like <code>no-images</code>. Control URL length and host parts.</p>
      </div>
      <div class="cell code-cell">
        <div class="snip"><span class="prompt">$</span> urx target.com \<br>&nbsp;&nbsp;&nbsp;&nbsp;-e js,php \<br>&nbsp;&nbsp;&nbsp;&nbsp;--patterns api,v1<br><br><span class="muted">https://target.com/api/v1/auth.js<br>https://target.com/api/config.php<br>https://target.com/v1/users.js</span></div>
      </div>
      <div class="cell">
        <div class="c-ico"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3v5h5M14 3l5 5v11a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1z"/><path d="M8 13h8M8 17h5"/></svg></div>
        <h3>Flexible output</h3>
        <p>Plain text, JSON, or CSV. Stream to the console, a file, or a pipe.</p>
      </div>
      <div class="cell">
        <div class="c-ico"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l7 3v6c0 4-3 6.5-7 9-4-2.5-7-5-7-9V6z"/><path d="M9 12l2 2 4-4"/></svg></div>
        <h3>URL validation</h3>
        <p>Check HTTP status codes, drop dead links, and extract more URLs from live pages.</p>
      </div>
      <div class="cell wide">
        <div class="c-ico"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><ellipse cx="12" cy="5" rx="8" ry="3"/><path d="M4 5v6c0 1.7 3.6 3 8 3s8-1.3 8-3V5M4 11v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6"/></svg></div>
        <h3>Caching &amp; incremental scanning</h3>
        <p>Local SQLite or remote Redis caching skips domains you already scanned. Incremental mode returns only the URLs discovered since last run.</p>
      </div>
      <div class="cell">
        <div class="c-ico"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2L4.5 13H11l-1 9 8.5-11H12z"/></svg></div>
        <h3>Built in Rust</h3>
        <p>Async runtime, parallel requests, and a small footprint for maximum throughput.</p>
      </div>
    </div>
  </div>
</section>

<section class="section">
  <div class="wrap">
    <div class="section-head reveal">
      <h2 class="section-title">From a domain to a clean URL set.</h2>
      <p class="section-sub">One command runs the full pipeline. Each stage maps to a flag you can tune.</p>
    </div>
    <div class="pipeline reveal">
      <div class="stage">
        <span class="s-idx">--providers</span>
        <h3>Collect</h3>
        <p>Query every archive in parallel and merge the results into one stream.</p>
      </div>
      <div class="stage">
        <span class="s-idx">-e · --patterns</span>
        <h3>Filter</h3>
        <p>Keep only the extensions, patterns, or presets you care about.</p>
      </div>
      <div class="stage">
        <span class="s-idx">--check-status</span>
        <h3>Validate</h3>
        <p>Probe live URLs, check HTTP status, and pull extra links from responses.</p>
      </div>
      <div class="stage">
        <span class="s-idx">-f json · -o</span>
        <h3>Output</h3>
        <p>Write text, JSON, or CSV to a file, or pipe it straight into your next tool.</p>
      </div>
    </div>
  </div>
</section>

<section class="section">
  <div class="wrap">
    <div class="launch reveal">
      <div class="starfield" aria-hidden="true"><div class="stars"></div></div>
      <div class="launch-inner">
        <span class="eyebrow" style="justify-content:center">Ready to launch</span>
        <h2>Install urx and start collecting.</h2>
        <p>Available on Cargo, Homebrew, and as a container image. No account, no key to get going.</p>
        <div class="install-cmds">
          <div class="cmd"><span class="prompt">$</span> cargo install urx <span class="via">Cargo</span></div>
          <div class="cmd"><span class="prompt">$</span> brew install urx <span class="via">Homebrew</span></div>
          <div class="cmd"><span class="prompt">$</span> docker pull ghcr.io/hahwul/urx <span class="via">Docker</span></div>
        </div>
        <a href="/getting-started/" class="btn btn-primary">Read the docs</a>
      </div>
    </div>
  </div>
</section>
