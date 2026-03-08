import { useState, type Dispatch } from "react";
import type { ChainConfig } from "../types";
import { EndpointList } from "./EndpointList";
import { HealthSection } from "./HealthSection";
import { CacheSection } from "./CacheSection";
import styles from "./ChainCard.module.css";

interface Props {
  chain: ChainConfig;
  chainIndex: number;
  canRemove: boolean;
  dispatch: Dispatch<any>;
}

export function ChainCard({ chain, chainIndex, canRemove, dispatch }: Props) {
  const [healthOpen, setHealthOpen] = useState(false);
  const [cacheOpen, setCacheOpen] = useState(false);

  return (
    <div className={styles.card}>
      <div className={styles.header}>
        <span className={styles.chainLabel}>Chain {chainIndex + 1}</span>
        {canRemove && (
          <button
            className={styles.removeBtn}
            onClick={() => dispatch({ type: "REMOVE_CHAIN", chainIndex })}
          >
            Remove
          </button>
        )}
      </div>

      <div className={styles.topRow}>
        <div className={styles.field}>
          <label>Name</label>
          <input
            type="text"
            value={chain.name}
            onChange={(e) =>
              dispatch({
                type: "SET_CHAIN_NAME",
                chainIndex,
                payload: e.target.value,
              })
            }
            placeholder="ethereum"
          />
        </div>
        <div className={styles.field}>
          <label>Route</label>
          <input
            type="text"
            value={chain.route}
            onChange={(e) =>
              dispatch({
                type: "SET_CHAIN_ROUTE",
                chainIndex,
                payload: e.target.value,
              })
            }
            placeholder="/ethereum"
          />
        </div>
        <div className={styles.fieldSmall}>
          <label>Rotation</label>
          <select
            value={chain.rotation}
            onChange={(e) =>
              dispatch({
                type: "SET_CHAIN_ROTATION",
                chainIndex,
                payload: e.target.value,
              })
            }
          >
            <option value="round_robin">Round Robin</option>
            <option value="weighted">Weighted</option>
          </select>
        </div>
      </div>

      <EndpointList
        endpoints={chain.endpoints}
        chainIndex={chainIndex}
        showWeight={chain.rotation === "weighted"}
        dispatch={dispatch}
      />

      <div className={styles.collapsible}>
        <button
          className={styles.collapseBtn}
          onClick={() => setHealthOpen(!healthOpen)}
        >
          <span className={`${styles.arrow} ${healthOpen ? styles.arrowOpen : ""}`}>
            &#9654;
          </span>
          Health Check
        </button>
        {healthOpen && (
          <HealthSection
            health={chain.health}
            chainName={chain.name}
            chainIndex={chainIndex}
            dispatch={dispatch}
          />
        )}
      </div>

      <div className={styles.collapsible}>
        <button
          className={styles.collapseBtn}
          onClick={() => setCacheOpen(!cacheOpen)}
        >
          <span className={`${styles.arrow} ${cacheOpen ? styles.arrowOpen : ""}`}>
            &#9654;
          </span>
          Cache
          {chain.cache && (
            <span className={styles.badge}>ON</span>
          )}
        </button>
        {cacheOpen && (
          <CacheSection
            cache={chain.cache}
            chainName={chain.name}
            chainIndex={chainIndex}
            dispatch={dispatch}
          />
        )}
      </div>
    </div>
  );
}
