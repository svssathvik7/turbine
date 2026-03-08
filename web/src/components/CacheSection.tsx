import type { Dispatch } from "react";
import type { CacheConfig } from "../types";
import { getDefaultCachePreset } from "../defaults";
import styles from "./CacheSection.module.css";

interface Props {
  cache: CacheConfig | null;
  chainName: string;
  chainIndex: number;
  dispatch: Dispatch<any>;
}

export function CacheSection({ cache, chainName, chainIndex, dispatch }: Props) {
  const defaultPreset = getDefaultCachePreset(chainName);

  return (
    <div className={styles.section}>
      <div className={styles.toggleRow}>
        <label className={styles.toggleLabel}>Enable Cache</label>
        <button
          className={`${styles.toggle} ${cache ? styles.toggleOn : ""}`}
          onClick={() => dispatch({ type: "TOGGLE_CACHE", chainIndex })}
        >
          <span className={styles.toggleKnob} />
        </button>
      </div>

      {cache && (
        <div className={styles.content}>
          <div className={styles.grid}>
            <div className={styles.field}>
              <label>Preset {!chainName && `(default: ${defaultPreset})`}</label>
              <select
                value={cache.preset}
                onChange={(e) =>
                  dispatch({
                    type: "SET_CACHE_PRESET",
                    chainIndex,
                    payload: e.target.value,
                  })
                }
              >
                <option value="evm">EVM</option>
                <option value="solana">Solana</option>
                <option value="none">None</option>
              </select>
            </div>
            <div className={styles.field}>
              <label>Max Capacity</label>
              <input
                type="number"
                value={cache.max_capacity}
                min={1}
                onChange={(e) =>
                  dispatch({
                    type: "SET_CACHE_CAPACITY",
                    chainIndex,
                    payload: parseInt(e.target.value) || 1,
                  })
                }
              />
            </div>
          </div>

          <div className={styles.methods}>
            <label>Method TTL Overrides</label>
            {cache.methods.map((m, i) => (
              <div key={i} className={styles.methodRow}>
                <input
                  type="text"
                  value={m.name}
                  onChange={(e) =>
                    dispatch({
                      type: "SET_CACHE_METHOD_NAME",
                      chainIndex,
                      methodIndex: i,
                      payload: e.target.value,
                    })
                  }
                  placeholder="e.g. eth_getBlockByNumber"
                />
                <input
                  type="number"
                  className={styles.ttlInput}
                  value={m.ttl_seconds}
                  min={0}
                  onChange={(e) =>
                    dispatch({
                      type: "SET_CACHE_METHOD_TTL",
                      chainIndex,
                      methodIndex: i,
                      payload: parseInt(e.target.value) || 0,
                    })
                  }
                  placeholder="TTL (s)"
                />
                <button
                  className={styles.removeBtn}
                  onClick={() =>
                    dispatch({
                      type: "REMOVE_CACHE_METHOD",
                      chainIndex,
                      methodIndex: i,
                    })
                  }
                >
                  &times;
                </button>
              </div>
            ))}
            <button
              className={styles.addBtn}
              onClick={() => dispatch({ type: "ADD_CACHE_METHOD", chainIndex })}
            >
              + Add Method Override
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
