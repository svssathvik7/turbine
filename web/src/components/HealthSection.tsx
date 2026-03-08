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
          <label>Health Method</label>
          <input
            type="text"
            value={health.health_method}
            onChange={(e) => setField("health_method", e.target.value)}
            placeholder={`auto: ${placeholder}`}
          />
        </div>
        <div className={styles.field}>
          <label>Max Consecutive Failures</label>
          <input
            type="number"
            value={health.max_consecutive_failures}
            min={1}
            onChange={(e) =>
              setField("max_consecutive_failures", parseInt(e.target.value) || 1)
            }
          />
        </div>
        <div className={styles.field}>
          <label>Cooldown (seconds)</label>
          <input
            type="number"
            value={health.cooldown_seconds}
            min={0}
            onChange={(e) =>
              setField("cooldown_seconds", parseInt(e.target.value) || 0)
            }
          />
        </div>
        <div className={styles.field}>
          <label>Check Interval (seconds)</label>
          <input
            type="number"
            value={health.health_check_interval_seconds}
            min={1}
            onChange={(e) =>
              setField("health_check_interval_seconds", parseInt(e.target.value) || 1)
            }
          />
        </div>
        <div className={styles.field}>
          <label>Max Block Lag</label>
          <input
            type="number"
            value={health.max_block_lag}
            min={0}
            onChange={(e) =>
              setField("max_block_lag", parseInt(e.target.value) || 0)
            }
          />
        </div>
      </div>
    </div>
  );
}
