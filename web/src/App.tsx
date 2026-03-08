import { Zap, Download, RotateCcw, Plus } from "lucide-react";
import { createDefaultConfig } from "./defaults";
import { useConfig } from "./hooks/useConfig";
import { ServerSection } from "./components/ServerSection";
import { ChainCard } from "./components/ChainCard";
import { TomlPreview } from "./components/TomlPreview";
import styles from "./App.module.css";

export default function App() {
  const { config, dispatch, toml, handleDownload } = useConfig(createDefaultConfig());

  return (
    <div className={styles.layout}>
      <header className={styles.header}>
        <div className={styles.headerLeft}>
          <div className={styles.logoIcon}>
            <Zap size={18} />
          </div>
          <div>
            <h1 className={styles.logo}>Turbine</h1>
            <span className={styles.subtitle}>RPC Proxy Config Generator</span>
          </div>
        </div>
        <div className={styles.headerRight}>
          <button className={styles.downloadBtn} onClick={handleDownload}>
            <Download size={15} />
            Download TOML
          </button>
          <button
            className={styles.resetBtn}
            onClick={() => dispatch({ type: "RESET_CONFIG" })}
          >
            <RotateCcw size={14} />
            Reset
          </button>
        </div>
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
            <Plus size={16} className={styles.addChainIcon} />
            Add Chain
          </button>
        </div>

        <div className={styles.preview}>
          <TomlPreview toml={toml} onDownload={handleDownload} />
        </div>
      </div>
    </div>
  );
}
