import type { Dispatch } from "react";
import { Link, X, Plus } from "lucide-react";
import type { EndpointConfig } from "../types";
import styles from "./EndpointList.module.css";

interface Props {
  endpoints: EndpointConfig[];
  chainIndex: number;
  showWeight: boolean;
  dispatch: Dispatch<any>;
}

export function EndpointList({ endpoints, chainIndex, showWeight, dispatch }: Props) {
  return (
    <div className={styles.section}>
      <div className={styles.header}>
        <label>Endpoints</label>
      </div>
      {endpoints.map((ep, i) => (
        <div key={i} className={styles.row}>
          <Link size={14} className={styles.linkIcon} />
          <input
            type="text"
            className={styles.urlInput}
            value={ep.url}
            onChange={(e) =>
              dispatch({
                type: "SET_ENDPOINT_URL",
                chainIndex,
                endpointIndex: i,
                payload: e.target.value,
              })
            }
            placeholder="https://rpc-endpoint.example.com"
          />
          {showWeight && (
            <div className={styles.weightField}>
              <span className={styles.weightLabel}>W</span>
              <input
                type="number"
                className={styles.weightInput}
                value={ep.weight}
                min={0}
                onChange={(e) =>
                  dispatch({
                    type: "SET_ENDPOINT_WEIGHT",
                    chainIndex,
                    endpointIndex: i,
                    payload: parseInt(e.target.value) || 0,
                  })
                }
                placeholder="1"
              />
            </div>
          )}
          <button
            className={styles.removeBtn}
            onClick={() =>
              dispatch({ type: "REMOVE_ENDPOINT", chainIndex, endpointIndex: i })
            }
            disabled={endpoints.length <= 1}
            aria-label="Remove endpoint"
          >
            <X size={14} />
          </button>
        </div>
      ))}
      <button
        className={styles.addBtn}
        onClick={() => dispatch({ type: "ADD_ENDPOINT", chainIndex })}
      >
        <Plus size={14} />
        Add Endpoint
      </button>
    </div>
  );
}
