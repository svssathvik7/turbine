pub const DASHBOARD_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Turbine Dashboard</title>
<style>
  :root {
    --bg: #0d1117;
    --card: #161b22;
    --border: #30363d;
    --text: #e6edf3;
    --text-dim: #8b949e;
    --green: #3fb950;
    --red: #f85149;
    --yellow: #d29922;
    --blue: #58a6ff;
    --purple: #bc8cff;
    --cyan: #39d2c0;
  }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, Oxygen, Ubuntu, sans-serif;
    background: var(--bg);
    color: var(--text);
    padding: 24px;
    min-height: 100vh;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 24px;
    flex-wrap: wrap;
    gap: 12px;
  }
  header h1 {
    font-size: 24px;
    font-weight: 700;
    display: flex;
    align-items: center;
    gap: 10px;
  }
  header h1 .logo {
    width: 28px; height: 28px;
    background: linear-gradient(135deg, var(--cyan), var(--blue));
    border-radius: 6px;
    display: inline-block;
  }
  .header-meta {
    display: flex;
    gap: 16px;
    align-items: center;
    color: var(--text-dim);
    font-size: 13px;
  }
  .pulse {
    width: 8px; height: 8px;
    background: var(--green);
    border-radius: 50%;
    display: inline-block;
    animation: pulse 2s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
  .overview {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 16px;
    margin-bottom: 32px;
  }
  .stat-card {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 20px;
  }
  .stat-card .label {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-dim);
    margin-bottom: 8px;
  }
  .stat-card .value {
    font-size: 32px;
    font-weight: 700;
    line-height: 1;
  }
  .stat-card .sub {
    font-size: 12px;
    color: var(--text-dim);
    margin-top: 6px;
  }
  .chains-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(480px, 1fr));
    gap: 16px;
  }
  .chain-card {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
    transition: border-color 0.2s;
  }
  .chain-card:hover { border-color: var(--blue); }
  .chain-header {
    padding: 16px 20px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--border);
  }
  .chain-name {
    font-size: 16px;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .chain-route {
    font-size: 12px;
    color: var(--text-dim);
    font-family: monospace;
  }
  .badge {
    font-size: 11px;
    padding: 3px 8px;
    border-radius: 12px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }
  .badge-rr { background: rgba(88,166,255,0.15); color: var(--blue); }
  .badge-w { background: rgba(188,140,255,0.15); color: var(--purple); }
  .badge-healthy { background: rgba(63,185,80,0.15); color: var(--green); }
  .badge-degraded { background: rgba(210,153,34,0.15); color: var(--yellow); }
  .badge-down { background: rgba(248,81,73,0.15); color: var(--red); }
  .chain-stats {
    padding: 16px 20px;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 12px;
    border-bottom: 1px solid var(--border);
  }
  .mini-stat .ms-label {
    font-size: 11px;
    color: var(--text-dim);
    margin-bottom: 4px;
  }
  .mini-stat .ms-value {
    font-size: 18px;
    font-weight: 600;
  }
  .progress-bar {
    height: 4px;
    background: var(--border);
    border-radius: 2px;
    margin-top: 6px;
    overflow: hidden;
  }
  .progress-fill {
    height: 100%;
    border-radius: 2px;
    transition: width 0.5s ease;
  }
  .endpoints-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  .endpoints-table th {
    text-align: left;
    padding: 8px 12px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-dim);
    border-bottom: 1px solid var(--border);
    font-weight: 500;
  }
  .endpoints-table td {
    padding: 8px 12px;
    border-bottom: 1px solid rgba(48,54,61,0.5);
    vertical-align: middle;
  }
  .endpoints-table tr:last-child td { border-bottom: none; }
  .endpoints-table tr:hover td { background: rgba(88,166,255,0.04); }
  .status-dot {
    width: 8px; height: 8px;
    border-radius: 50%;
    display: inline-block;
    margin-right: 6px;
    flex-shrink: 0;
  }
  .dot-healthy { background: var(--green); }
  .dot-unhealthy { background: var(--red); }
  .url-cell {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .url-chip {
    font-family: monospace;
    font-size: 11px;
    padding: 2px 8px;
    background: rgba(88,166,255,0.1);
    border: 1px solid rgba(88,166,255,0.2);
    border-radius: 4px;
    cursor: pointer;
    white-space: nowrap;
    transition: background 0.15s;
    position: relative;
  }
  .url-chip:hover { background: rgba(88,166,255,0.2); }
  .copy-tooltip {
    position: absolute;
    top: -28px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--green);
    color: #000;
    font-size: 10px;
    font-weight: 600;
    padding: 3px 8px;
    border-radius: 4px;
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.2s;
  }
  .copy-tooltip.show { opacity: 1; }
  .latency-good { color: var(--green); }
  .latency-ok { color: var(--yellow); }
  .latency-bad { color: var(--red); }
  .na { color: var(--text-dim); font-style: italic; }
  .endpoint-bar {
    display: flex;
    gap: 2px;
    margin-bottom: 2px;
  }
  .endpoint-bar .seg {
    height: 20px;
    border-radius: 3px;
    min-width: 4px;
    transition: flex-grow 0.5s;
  }
  #loading {
    text-align: center;
    padding: 80px;
    color: var(--text-dim);
    font-size: 16px;
  }
  @media (max-width: 768px) {
    .overview { grid-template-columns: repeat(2, 1fr); }
    .chains-grid { grid-template-columns: 1fr; }
    .chain-stats { grid-template-columns: repeat(2, 1fr); }
    body { padding: 16px; }
  }
  @media (max-width: 480px) {
    .overview { grid-template-columns: 1fr; }
    .chain-stats { grid-template-columns: 1fr; }
  }
</style>
</head>
<body>
<header>
  <h1><span class="logo"></span> Turbine Dashboard</h1>
  <div class="header-meta">
    <span><span class="pulse"></span> Live</span>
    <span id="uptime">Uptime: --</span>
    <span id="updated">Updated: --</span>
  </div>
</header>
<div class="overview" id="overview"></div>
<div class="chains-grid" id="chains"></div>
<div id="loading">Connecting to Turbine...</div>
<script>
let lastData = null;

function formatUptime(secs) {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  let parts = [];
  if (d > 0) parts.push(d + 'd');
  if (h > 0) parts.push(h + 'h');
  if (m > 0) parts.push(m + 'm');
  parts.push(s + 's');
  return parts.join(' ');
}

function formatNum(n) {
  if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M';
  if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K';
  return n.toString();
}

function pct(a, b) {
  if (b === 0) return 0;
  return ((a / b) * 100).toFixed(1);
}

function latencyClass(ms) {
  if (ms === null || ms === undefined) return 'na';
  if (ms < 200) return 'latency-good';
  if (ms < 1000) return 'latency-ok';
  return 'latency-bad';
}

function latencyColor(ms) {
  if (ms === null || ms === undefined) return 'var(--text-dim)';
  if (ms < 200) return 'var(--green)';
  if (ms < 1000) return 'var(--yellow)';
  return 'var(--red)';
}

function chainHealth(c) {
  if (c.active_endpoints === c.total_endpoints) return 'healthy';
  if (c.active_endpoints === 0) return 'down';
  return 'degraded';
}

function shortLabel(url) {
  try {
    const u = new URL(url);
    const parts = u.hostname.replace(/^www\./, '').split('.');
    const name = parts[0].length > 5 ? parts[0].slice(0, 5) : parts[0];
    return name;
  } catch {
    return url.slice(0, 5);
  }
}

function formatBlockHeight(n) {
  if (n === null || n === undefined) return '--';
  return n.toLocaleString();
}

function copyUrl(el, url) {
  navigator.clipboard.writeText(url);
  const tip = el.querySelector('.copy-tooltip');
  tip.classList.add('show');
  setTimeout(() => tip.classList.remove('show'), 1200);
}

function render(data) {
  document.getElementById('loading').style.display = 'none';
  document.getElementById('uptime').textContent = 'Uptime: ' + formatUptime(data.uptime_seconds);
  document.getElementById('updated').textContent = 'Updated: just now';

  // Overview
  const totalReqs = data.chains.reduce((s, c) => s + c.total_requests, 0);
  const totalSuccess = data.chains.reduce((s, c) => s + c.successful_requests, 0);
  const totalEndpoints = data.chains.reduce((s, c) => s + c.total_endpoints, 0);
  const activeEndpoints = data.chains.reduce((s, c) => s + c.active_endpoints, 0);
  const totalCacheHits = data.chains.reduce((s, c) => s + c.cache_hits, 0);
  const totalCacheMisses = data.chains.reduce((s, c) => s + c.cache_misses, 0);
  const cacheTotal = totalCacheHits + totalCacheMisses;

  document.getElementById('overview').innerHTML = `
    <div class="stat-card">
      <div class="label">Total Requests</div>
      <div class="value">${formatNum(totalReqs)}</div>
      <div class="sub">${formatNum(totalSuccess)} successful</div>
    </div>
    <div class="stat-card">
      <div class="label">Success Rate</div>
      <div class="value" style="color:${totalReqs > 0 ? (totalSuccess/totalReqs > 0.95 ? 'var(--green)' : totalSuccess/totalReqs > 0.8 ? 'var(--yellow)' : 'var(--red)') : 'var(--text-dim)'}">${totalReqs > 0 ? pct(totalSuccess, totalReqs) + '%' : '--'}</div>
      <div class="sub">${formatNum(totalReqs - totalSuccess)} failed</div>
    </div>
    <div class="stat-card">
      <div class="label">Active Chains</div>
      <div class="value">${data.chains.length}</div>
      <div class="sub">${activeEndpoints}/${totalEndpoints} endpoints healthy</div>
    </div>
    <div class="stat-card">
      <div class="label">Cache Hit Rate</div>
      <div class="value" style="color:${cacheTotal > 0 ? 'var(--cyan)' : 'var(--text-dim)'}">${cacheTotal > 0 ? pct(totalCacheHits, cacheTotal) + '%' : '--'}</div>
      <div class="sub">${formatNum(totalCacheHits)} hits / ${formatNum(totalCacheMisses)} misses</div>
    </div>
    ${(() => {
      const totalWsActive = data.chains.reduce((s, c) => s + (c.ws_active_connections || 0), 0);
      const totalWsTotal = data.chains.reduce((s, c) => s + (c.ws_connections_total || 0), 0);
      if (totalWsTotal > 0) {
        return `<div class="stat-card">
          <div class="label">WebSocket</div>
          <div class="value" style="color:var(--cyan)">${formatNum(totalWsActive)}</div>
          <div class="sub">${formatNum(totalWsTotal)} total connections</div>
        </div>`;
      }
      return '';
    })()}
  `;

  // Chains
  let chainsHtml = '';
  for (const c of data.chains) {
    const health = chainHealth(c);
    const healthBadge = health === 'healthy'
      ? '<span class="badge badge-healthy">Healthy</span>'
      : health === 'degraded'
      ? '<span class="badge badge-degraded">Degraded</span>'
      : '<span class="badge badge-down">Down</span>';

    const rotBadge = c.rotation === 'weighted'
      ? '<span class="badge badge-w">Weighted</span>'
      : '<span class="badge badge-rr">Round Robin</span>';

    const successRate = c.total_requests > 0 ? pct(c.successful_requests, c.total_requests) : 0;
    const cacheHitRate = (c.cache_hits + c.cache_misses) > 0
      ? pct(c.cache_hits, c.cache_hits + c.cache_misses) : 0;

    const successColor = successRate >= 95 ? 'var(--green)' : successRate >= 80 ? 'var(--yellow)' : 'var(--red)';
    const cacheColor = 'var(--cyan)';

    // Endpoint bar visualization
    let barHtml = '<div class="endpoint-bar">';
    for (const ep of c.endpoints) {
      const color = ep.is_healthy ? 'var(--green)' : 'var(--red)';
      const grow = Math.max(ep.weight, 1);
      barHtml += `<div class="seg" style="flex-grow:${grow};background:${color}" title="${ep.url}: ${ep.is_healthy ? 'healthy' : 'unhealthy'}"></div>`;
    }
    barHtml += '</div>';

    let endpointsHtml = '';
    for (const ep of c.endpoints) {
      const dot = ep.is_healthy ? 'dot-healthy' : 'dot-unhealthy';
      const latency = ep.rolling_latency_ms !== null && ep.rolling_latency_ms !== undefined
        ? Math.round(ep.rolling_latency_ms) + 'ms'
        : '--';
      const lClass = latencyClass(ep.rolling_latency_ms);
      const block = formatBlockHeight(ep.block_height);
      const epSuccessRate = ep.request_count > 0
        ? pct(ep.success_count, ep.request_count) + '%'
        : '--';
      const isReserve = ep.roster_status === 'reserve';
      const rowStyle = isReserve ? ' style="opacity:0.4"' : '';
      const badge = isReserve ? ' <span style="color:var(--text-dim);font-size:0.75em">[RESERVE]</span>' : '';

      endpointsHtml += `<tr${rowStyle}>
        <td><div class="url-cell"><span class="status-dot ${dot}"></span><span class="url-chip" onclick="copyUrl(this, '${ep.url.replace(/'/g, "\\'")}')" title="${ep.url}">${shortLabel(ep.url)}<span class="copy-tooltip">Copied!</span></span>${badge}</div></td>
        <td class="${lClass}">${latency}</td>
        <td>${block}</td>
        <td style="text-align:center">${ep.weight}</td>
        <td style="text-align:center">${ep.request_count}</td>
        <td style="text-align:center">${epSuccessRate}</td>
        <td style="text-align:center">${ep.consecutive_failures > 0 ? '<span style="color:var(--red)">' + ep.consecutive_failures + '</span>' : '0'}</td>
      </tr>`;
    }

    chainsHtml += `
    <div class="chain-card">
      <div class="chain-header">
        <div>
          <div class="chain-name">${c.name} ${healthBadge}</div>
          <div class="chain-route">${c.route}</div>
        </div>
        <div>${rotBadge}</div>
      </div>
      <div class="chain-stats">
        <div class="mini-stat">
          <div class="ms-label">Requests</div>
          <div class="ms-value">${formatNum(c.total_requests)}</div>
        </div>
        <div class="mini-stat">
          <div class="ms-label">Success Rate</div>
          <div class="ms-value" style="color:${c.total_requests > 0 ? successColor : 'var(--text-dim)'}">${c.total_requests > 0 ? successRate + '%' : '--'}</div>
          <div class="progress-bar"><div class="progress-fill" style="width:${successRate}%;background:${successColor}"></div></div>
        </div>
        <div class="mini-stat">
          <div class="ms-label">Endpoints</div>
          <div class="ms-value">${c.active_endpoints}<span style="color:var(--text-dim);font-size:14px">/${c.total_endpoints}</span></div>
        </div>
      </div>
      ${(c.ws_connections_total || 0) > 0 ? `<div class="chain-stats">
        <div class="mini-stat">
          <div class="ms-label">WS Active</div>
          <div class="ms-value" style="color:var(--cyan)">${formatNum(c.ws_active_connections || 0)}</div>
        </div>
        <div class="mini-stat">
          <div class="ms-label">WS Total</div>
          <div class="ms-value">${formatNum(c.ws_connections_total || 0)}</div>
        </div>
        <div class="mini-stat">
          <div class="ms-label">WS Messages</div>
          <div class="ms-value">${formatNum(c.ws_messages_relayed || 0)}</div>
        </div>
      </div>` : ''}
      <div style="padding:12px 20px 8px">
        ${barHtml}
      </div>
      <table class="endpoints-table">
        <thead>
          <tr>
            <th>Endpoint</th>
            <th>Latency</th>
            <th>Block</th>
            <th style="text-align:center">Weight</th>
            <th style="text-align:center">Reqs</th>
            <th style="text-align:center">Success</th>
            <th style="text-align:center">Fails</th>
          </tr>
        </thead>
        <tbody>${endpointsHtml}</tbody>
      </table>
    </div>`;
  }

  document.getElementById('chains').innerHTML = chainsHtml;
}

async function fetchStatus() {
  try {
    const resp = await fetch('/api/status');
    if (!resp.ok) throw new Error('HTTP ' + resp.status);
    const data = await resp.json();
    lastData = data;
    render(data);
  } catch (e) {
    console.error('Failed to fetch status:', e);
    document.getElementById('loading').style.display = 'block';
    document.getElementById('loading').textContent = 'Connection lost. Retrying...';
  }
}

fetchStatus();
setInterval(fetchStatus, 5000);
</script>
</body>
</html>"##;
