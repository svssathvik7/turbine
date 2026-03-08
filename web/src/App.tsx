import { createDefaultConfig } from "./defaults";
import { useConfig } from "./hooks/useConfig";
import { ServerSection } from "./components/ServerSection";
import { ChainCard } from "./components/ChainCard";
import { TomlPreview } from "./components/TomlPreview";
import styles from "./App.module.css";

export default function App() {
  const { config, dispatch, toml } = useConfig(createDefaultConfig());

  return (
    <div className={styles.layout}>
      <header className={styles.header}>
        <h1 className={styles.logo}>Turbine</h1>
        <span className={styles.subtitle}>Config Generator</span>
      </header>

      <div className={styles.content}>
        <div className={styles.form}>
          <ServerSection server={config.server} dispatch={dispatch} />

          <div className={styles.chainList}>
            {config.chains.map((chain, i) => (
              <ChainCard
                key={i}
                chain={chain}
                chainIndex={i}
                canRemove={config.chains.length > 1}
                dispatch={dispatch}
              />
            ))}
          </div>

          <button
            className={styles.addChainBtn}
            onClick={() => dispatch({ type: "ADD_CHAIN" })}
          >
            + Add Chain
          </button>
        </div>

        <div className={styles.preview}>
          <TomlPreview toml={toml} />
        </div>
      </div>
    </div>
  );
}
