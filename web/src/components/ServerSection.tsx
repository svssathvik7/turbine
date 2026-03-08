import type { Dispatch } from "react";
import type { ServerConfig } from "../types";
import styles from "./ServerSection.module.css";

interface Props {
  server: ServerConfig;
  dispatch: Dispatch<any>;
}

export function ServerSection({ server, dispatch }: Props) {
  return (
    <div className={styles.section}>
      <h3 className={styles.title}>Server</h3>
      <div className={styles.row}>
        <div className={styles.field}>
          <label>Host</label>
          <input
            type="text"
            value={server.host}
            onChange={(e) =>
              dispatch({ type: "SET_SERVER_HOST", payload: e.target.value })
            }
            placeholder="127.0.0.1"
          />
        </div>
        <div className={styles.fieldSmall}>
          <label>Port</label>
          <input
            type="number"
            value={server.port}
            onChange={(e) =>
              dispatch({
                type: "SET_SERVER_PORT",
                payload: parseInt(e.target.value) || 0,
              })
            }
            placeholder="8080"
          />
        </div>
      </div>
    </div>
  );
}
