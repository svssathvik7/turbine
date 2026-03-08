import type { Dispatch } from "react";
import type { HealthConfig } from "../types";
import { getDefaultHealthMethod } from "../defaults";
import styles from "./HealthSection.module.css";

interface Props {
  health: HealthConfig;
  chainName: string;
  chainIndex: number;
  dispatch: Dispatch<any>;
}

export function HealthSection({ health, chainName, chainIndex, dispatch }: Props) {
  const setField = (field: string, value: string | number) =>
    dispatch({ type: "SET_HEALTH_FIELD", chainIndex, field, payload: value });

  const placeholder = getDefaultHealthMethod(chainName);

  return (
    <div className={styles.section}>
      <div className={styles.grid}>
        <div className={styles.field}>
          <label htmlFor={`health-${chainIndex}-method`}>Health Method</label>
          <input
            id={`health-${chainIndex}-method`}
            type="text"
            value={health.health_method}
            onChange={(e) => setField("health_method", e.target.value)}
            placeholder={`auto: ${placeholder}`}
            aria-describedby={`health-${chainIndex}-method-hint`}
          />
          <span id={`health-${chainIndex}-method-hint`} className={styles.hint}>
            RPC method used for health checks
          </span>
        </div>
        <div className={styles.field}>
          <label htmlFor={`health-${chainIndex}-failures`}>Max Consecutive Failures</label>
          <input
            id={`health-${chainIndex}-failures`}
            type="number"
            value={health.max_consecutive_failures}
            min={1}
            onChange={(e) =>
              setField("max_consecutive_failures", parseInt(e.target.value) || 1)
            }
            aria-describedby={`health-${chainIndex}-failures-hint`}
          />
          <span id={`health-${chainIndex}-failures-hint`} className={styles.hint}>
            Requests before marking unhealthy
          </span>
        </div>
        <div className={styles.field}>
          <label htmlFor={`health-${chainIndex}-cooldown`}>Cooldown (seconds)</label>
          <input
            id={`health-${chainIndex}-cooldown`}
            type="number"
            value={health.cooldown_seconds}
            min={0}
            onChange={(e) =>
              setField("cooldown_seconds", parseInt(e.target.value) || 0)
            }
            aria-describedby={`health-${chainIndex}-cooldown-hint`}
          />
          <span id={`health-${chainIndex}-cooldown-hint`} className={styles.hint}>
            Wait time before retrying failed endpoint
          </span>
        </div>
        <div className={styles.field}>
          <label htmlFor={`health-${chainIndex}-interval`}>Check Interval (seconds)</label>
          <input
            id={`health-${chainIndex}-interval`}
            type="number"
            value={health.health_check_interval_seconds}
            min={1}
            onChange={(e) =>
              setField("health_check_interval_seconds", parseInt(e.target.value) || 1)
            }
            aria-describedby={`health-${chainIndex}-interval-hint`}
          />
          <span id={`health-${chainIndex}-interval-hint`} className={styles.hint}>
            Time between health check pings
          </span>
        </div>
        <div className={styles.field}>
          <label htmlFor={`health-${chainIndex}-blocklag`}>Max Block Lag</label>
          <input
            id={`health-${chainIndex}-blocklag`}
            type="number"
            value={health.max_block_lag}
            min={0}
            onChange={(e) =>
              setField("max_block_lag", parseInt(e.target.value) || 0)
            }
            aria-describedby={`health-${chainIndex}-blocklag-hint`}
          />
          <span id={`health-${chainIndex}-blocklag-hint`} className={styles.hint}>
            Blocks behind before marking stale
          </span>
        </div>
      </div>
    </div>
  );
}
