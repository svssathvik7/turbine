import type { Dispatch } from "react";
import { Server } from "lucide-react";
import type { ServerConfig } from "../types";
import styles from "./ServerSection.module.css";

interface Props {
  server: ServerConfig;
  dispatch: Dispatch<any>;
}

export function ServerSection({ server, dispatch }: Props) {
  return (
    <div className={styles.section}>
      <h3 className={styles.title}>
        <Server size={16} />
        Server
      </h3>
      <div className={styles.row}>
        <div className={styles.field}>
          <label htmlFor="server-host">Host</label>
          <input
            id="server-host"
            type="text"
            value={server.host}
            onChange={(e) =>
              dispatch({ type: "SET_SERVER_HOST", payload: e.target.value })
            }
            placeholder="127.0.0.1"
          />
        </div>
        <div className={styles.fieldSmall}>
          <label htmlFor="server-port">Port</label>
          <input
            id="server-port"
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
