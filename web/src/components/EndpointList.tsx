import type { Dispatch } from "react";
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
              placeholder="Weight"
            />
          )}
          <button
            className={styles.removeBtn}
            onClick={() =>
              dispatch({ type: "REMOVE_ENDPOINT", chainIndex, endpointIndex: i })
            }
            disabled={endpoints.length <= 1}
            title="Remove endpoint"
          >
            &times;
          </button>
        </div>
      ))}
      <button
        className={styles.addBtn}
        onClick={() => dispatch({ type: "ADD_ENDPOINT", chainIndex })}
      >
        + Add Endpoint
      </button>
    </div>
  );
}
