export interface TurbineConfig {
  server: ServerConfig;
  chains: ChainConfig[];
}

export interface ServerConfig {
  host: string;
  port: number;
}

export interface ChainConfig {
  name: string;
  route: string;
  rotation: "round_robin" | "weighted";
  endpoints: EndpointConfig[];
  health: HealthConfig;
  cache: CacheConfig | null;
}

export interface EndpointConfig {
  url: string;
  weight: number;
}

export interface HealthConfig {
  max_consecutive_failures: number;
  cooldown_seconds: number;
  health_method: string;
  health_check_interval_seconds: number;
  max_block_lag: number;
}

export interface CacheConfig {
  enabled: boolean;
  preset: "evm" | "solana" | "none";
  max_capacity: number;
  methods: CacheMethodConfig[];
}

export interface CacheMethodConfig {
  name: string;
  ttl_seconds: number;
}
