import type { TurbineConfig, ChainConfig } from "./types";

function escapeTomlString(s: string): string {
  return s.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
}

function q(s: string): string {
  return `"${escapeTomlString(s)}"`;
}

function serializeEndpoints(chain: ChainConfig): string {
  const items = chain.endpoints
    .filter((e) => e.url.trim())
    .map((e) => {
      if (chain.rotation === "weighted" && e.weight !== 1) {
        return `  { url = ${q(e.url)}, weight = ${e.weight} }`;
      }
      return `  ${q(e.url)}`;
    });

  if (items.length === 0) return "endpoints = []";
  return `endpoints = [\n${items.join(",\n")}\n]`;
}

function serializeChain(chain: ChainConfig): string {
  const lines: string[] = ["[[chains]]"];
  lines.push(`name = ${q(chain.name)}`);
  lines.push(`route = ${q(chain.route)}`);

  if (chain.rotation !== "round_robin") {
    lines.push(`rotation = ${q(chain.rotation)}`);
  }

  lines.push(serializeEndpoints(chain));

  // Health
  lines.push("");
  lines.push("[chains.health]");
  lines.push(
    `max_consecutive_failures = ${chain.health.max_consecutive_failures}`
  );
  lines.push(`cooldown_seconds = ${chain.health.cooldown_seconds}`);
  if (chain.health.health_method) {
    lines.push(`health_method = ${q(chain.health.health_method)}`);
  }
  lines.push(
    `health_check_interval_seconds = ${chain.health.health_check_interval_seconds}`
  );
  lines.push(`max_block_lag = ${chain.health.max_block_lag}`);

  // Cache
  if (chain.cache && chain.cache.enabled) {
    lines.push("");
    lines.push("[chains.cache]");
    lines.push(`enabled = true`);
    lines.push(`preset = ${q(chain.cache.preset)}`);
    lines.push(`max_capacity = ${chain.cache.max_capacity}`);

    for (const method of chain.cache.methods) {
      if (method.name.trim()) {
        lines.push("");
        lines.push("[[chains.cache.methods]]");
        lines.push(`name = ${q(method.name)}`);
        lines.push(`ttl_seconds = ${method.ttl_seconds}`);
      }
    }
  }

  return lines.join("\n");
}

export function serializeConfig(config: TurbineConfig): string {
  const lines: string[] = [
    "# Turbine RPC Proxy Configuration",
    "",
    "[server]",
    `host = ${q(config.server.host)}`,
    `port = ${config.server.port}`,
  ];

  for (const chain of config.chains) {
    lines.push("");
    lines.push(serializeChain(chain));
  }

  return lines.join("\n") + "\n";
}
