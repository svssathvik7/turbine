import { useState, type Dispatch } from "react";
import { ChevronRight, Trash2, Activity, Database } from "lucide-react";
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
        <div className={styles.headerLeft}>
          <span className={styles.chainBadge}>{chainIndex + 1}</span>
          <span className={styles.chainLabel}>
            {chain.name || `Chain ${chainIndex + 1}`}
          </span>
        </div>
        {canRemove && (
          <button
            className={styles.removeBtn}
            onClick={() => dispatch({ type: "REMOVE_CHAIN", chainIndex })}
            aria-label={`Remove chain ${chainIndex + 1}`}
          >
            <Trash2 size={15} />
          </button>
        )}
      </div>

      <div className={styles.topRow}>
        <div className={styles.field}>
          <label htmlFor={`chain-${chainIndex}-name`}>Name</label>
          <input
            id={`chain-${chainIndex}-name`}
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
          <label htmlFor={`chain-${chainIndex}-route`}>Route</label>
          <input
            id={`chain-${chainIndex}-route`}
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
          <label htmlFor={`chain-${chainIndex}-rotation`}>Rotation</label>
          <select
            id={`chain-${chainIndex}-rotation`}
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
          aria-expanded={healthOpen}
        >
          <ChevronRight
            size={14}
            className={`${styles.chevron} ${healthOpen ? styles.chevronOpen : ""}`}
          />
          <Activity size={14} className={styles.sectionIcon} />
          <span>Health Check</span>
          <span className={styles.sectionHint}>Monitoring & failover</span>
        </button>
        <div className={`${styles.collapseContent} ${healthOpen ? styles.collapseContentOpen : ""}`}>
          <div className={styles.collapseInner}>
            <HealthSection
              health={chain.health}
              chainName={chain.name}
              chainIndex={chainIndex}
              dispatch={dispatch}
            />
          </div>
        </div>
      </div>

      <div className={styles.collapsible}>
        <button
          className={styles.collapseBtn}
          onClick={() => setCacheOpen(!cacheOpen)}
          aria-expanded={cacheOpen}
        >
          <ChevronRight
            size={14}
            className={`${styles.chevron} ${cacheOpen ? styles.chevronOpen : ""}`}
          />
          <Database size={14} className={styles.sectionIcon} />
          <span>Cache</span>
          {chain.cache && (
            <span className={styles.badge}>ON</span>
          )}
          <span className={styles.sectionHint}>Response caching</span>
        </button>
        <div className={`${styles.collapseContent} ${cacheOpen ? styles.collapseContentOpen : ""}`}>
          <div className={styles.collapseInner}>
            <CacheSection
              cache={chain.cache}
              chainName={chain.name}
              chainIndex={chainIndex}
              dispatch={dispatch}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
