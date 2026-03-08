import { useReducer, useMemo, useEffect, useRef, useCallback } from "react";
import type { TurbineConfig } from "../types";
import { createDefaultChain, createDefaultCache, generateRoute, createDefaultConfig } from "../defaults";
import { serializeConfig } from "../toml";

const STORAGE_KEY = "turbine-config";

function loadFromStorage(): TurbineConfig | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw) as TurbineConfig;
  } catch {
    // ignore corrupt data
  }
  return null;
}

function saveToStorage(config: TurbineConfig): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
  } catch {
    // ignore quota errors
  }
}

export type Action =
  | { type: "SET_SERVER_HOST"; payload: string }
  | { type: "SET_SERVER_PORT"; payload: number }
  | { type: "ADD_CHAIN" }
  | { type: "REMOVE_CHAIN"; chainIndex: number }
  | { type: "SET_CHAIN_NAME"; chainIndex: number; payload: string }
  | { type: "SET_CHAIN_ROUTE"; chainIndex: number; payload: string }
  | { type: "SET_CHAIN_ROTATION"; chainIndex: number; payload: "round_robin" | "weighted" }
  | { type: "ADD_ENDPOINT"; chainIndex: number }
  | { type: "REMOVE_ENDPOINT"; chainIndex: number; endpointIndex: number }
  | { type: "SET_ENDPOINT_URL"; chainIndex: number; endpointIndex: number; payload: string }
  | { type: "SET_ENDPOINT_WEIGHT"; chainIndex: number; endpointIndex: number; payload: number }
  | { type: "SET_HEALTH_FIELD"; chainIndex: number; field: string; payload: string | number }
  | { type: "TOGGLE_CACHE"; chainIndex: number }
  | { type: "SET_CACHE_PRESET"; chainIndex: number; payload: "evm" | "solana" | "none" }
  | { type: "SET_CACHE_CAPACITY"; chainIndex: number; payload: number }
  | { type: "ADD_CACHE_METHOD"; chainIndex: number }
  | { type: "REMOVE_CACHE_METHOD"; chainIndex: number; methodIndex: number }
  | { type: "SET_CACHE_METHOD_NAME"; chainIndex: number; methodIndex: number; payload: string }
  | { type: "SET_CACHE_METHOD_TTL"; chainIndex: number; methodIndex: number; payload: number }
  | { type: "RESET_CONFIG" };

function updateChain(
  state: TurbineConfig,
  chainIndex: number,
  updater: (chain: TurbineConfig["chains"][0]) => TurbineConfig["chains"][0]
): TurbineConfig {
  return {
    ...state,
    chains: state.chains.map((c, i) => (i === chainIndex ? updater({ ...c }) : c)),
  };
}

function reducer(state: TurbineConfig, action: Action): TurbineConfig {
  switch (action.type) {
    case "SET_SERVER_HOST":
      return { ...state, server: { ...state.server, host: action.payload } };
    case "SET_SERVER_PORT":
      return { ...state, server: { ...state.server, port: action.payload } };

    case "ADD_CHAIN":
      return { ...state, chains: [...state.chains, createDefaultChain()] };
    case "REMOVE_CHAIN":
      return { ...state, chains: state.chains.filter((_, i) => i !== action.chainIndex) };

    case "SET_CHAIN_NAME":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        name: action.payload,
        route: generateRoute(action.payload),
      }));
    case "SET_CHAIN_ROUTE":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        route: action.payload,
      }));
    case "SET_CHAIN_ROTATION":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        rotation: action.payload,
      }));

    case "ADD_ENDPOINT":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        endpoints: [...c.endpoints, { url: "", weight: 1 }],
      }));
    case "REMOVE_ENDPOINT":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        endpoints: c.endpoints.filter((_, i) => i !== action.endpointIndex),
      }));
    case "SET_ENDPOINT_URL":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        endpoints: c.endpoints.map((e, i) =>
          i === action.endpointIndex ? { ...e, url: action.payload } : e
        ),
      }));
    case "SET_ENDPOINT_WEIGHT":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        endpoints: c.endpoints.map((e, i) =>
          i === action.endpointIndex ? { ...e, weight: action.payload } : e
        ),
      }));

    case "SET_HEALTH_FIELD":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        health: { ...c.health, [action.field]: action.payload },
      }));

    case "TOGGLE_CACHE":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        cache: c.cache ? null : createDefaultCache(c.name),
      }));
    case "SET_CACHE_PRESET":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        cache: c.cache ? { ...c.cache, preset: action.payload } : null,
      }));
    case "SET_CACHE_CAPACITY":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        cache: c.cache ? { ...c.cache, max_capacity: action.payload } : null,
      }));
    case "ADD_CACHE_METHOD":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        cache: c.cache
          ? { ...c.cache, methods: [...c.cache.methods, { name: "", ttl_seconds: 300 }] }
          : null,
      }));
    case "REMOVE_CACHE_METHOD":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        cache: c.cache
          ? { ...c.cache, methods: c.cache.methods.filter((_, i) => i !== action.methodIndex) }
          : null,
      }));
    case "SET_CACHE_METHOD_NAME":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        cache: c.cache
          ? {
              ...c.cache,
              methods: c.cache.methods.map((m, i) =>
                i === action.methodIndex ? { ...m, name: action.payload } : m
              ),
            }
          : null,
      }));
    case "SET_CACHE_METHOD_TTL":
      return updateChain(state, action.chainIndex, (c) => ({
        ...c,
        cache: c.cache
          ? {
              ...c.cache,
              methods: c.cache.methods.map((m, i) =>
                i === action.methodIndex ? { ...m, ttl_seconds: action.payload } : m
              ),
            }
          : null,
      }));

    case "RESET_CONFIG": {
      localStorage.removeItem(STORAGE_KEY);
      return createDefaultConfig();
    }

    default:
      return state;
  }
}

export function useConfig(initialConfig: TurbineConfig) {
  const [config, dispatch] = useReducer(reducer, initialConfig, () => {
    return loadFromStorage() ?? initialConfig;
  });

  const toml = useMemo(() => serializeConfig(config), [config]);

  // Debounced save to localStorage
  const timerRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => {
    clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => saveToStorage(config), 300);
    return () => clearTimeout(timerRef.current);
  }, [config]);

  const handleDownload = useCallback(() => {
    const blob = new Blob([toml], { type: "text/plain" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "config.toml";
    a.click();
    URL.revokeObjectURL(url);
  }, [toml]);

  return { config, dispatch, toml, handleDownload };
}
