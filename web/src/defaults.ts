import type { TurbineConfig, ChainConfig, CacheConfig } from "./types";

export function getDefaultHealthMethod(chainName: string): string {
  const name = chainName.toLowerCase();
  if (name.includes("solana")) return "getSlot";
  if (name.includes("starknet")) return "starknet_blockNumber";
  return "eth_blockNumber";
}

export function getDefaultCachePreset(
  chainName: string
): "evm" | "solana" | "none" {
  const name = chainName.toLowerCase();
  if (name.includes("solana")) return "solana";
  return "evm";
}

export function generateRoute(name: string): string {
  const slug = name.toLowerCase().replace(/\s+/g, "-");
  return slug ? `/${slug}` : "/";
}

export function createDefaultCache(chainName: string): CacheConfig {
  return {
    enabled: true,
    preset: getDefaultCachePreset(chainName),
    max_capacity: 10000,
    methods: [],
  };
}

export function createDefaultChain(): ChainConfig {
  return {
    name: "",
    route: "/",
    rotation: "round_robin",
    endpoints: [{ url: "", weight: 1 }],
    health: {
      max_consecutive_failures: 3,
      cooldown_seconds: 30,
      health_method: "",
      health_check_interval_seconds: 30,
      max_block_lag: 10,
    },
    cache: null,
  };
}

export function createDefaultConfig(): TurbineConfig {
  return {
    server: { host: "127.0.0.1", port: 8080 },
    chains: [createDefaultChain()],
  };
}
