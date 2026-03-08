import { useState } from "react";
import styles from "./TomlPreview.module.css";

interface Props {
  toml: string;
}

export function TomlPreview({ toml }: Props) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(toml);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <span className={styles.title}>config.toml</span>
        <button className={styles.copyBtn} onClick={handleCopy}>
          {copied ? "Copied!" : "Copy"}
        </button>
      </div>
      <pre className={styles.code}>
        <code>{toml}</code>
      </pre>
    </div>
  );
}
