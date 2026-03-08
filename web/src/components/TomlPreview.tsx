import { useState, useMemo } from "react";
import { FileCode, Copy, Check, Download } from "lucide-react";
import { highlightToml } from "../utils/highlightToml";
import styles from "./TomlPreview.module.css";

interface Props {
  toml: string;
  onDownload: () => void;
}

export function TomlPreview({ toml, onDownload }: Props) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    await navigator.clipboard.writeText(toml);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const highlighted = useMemo(() => highlightToml(toml), [toml]);
  const lineCount = toml.split("\n").length;

  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        <div className={styles.headerLeft}>
          <FileCode size={14} className={styles.fileIcon} />
          <span className={styles.title}>config.toml</span>
        </div>
        <div className={styles.headerRight}>
          <button
            className={`${styles.actionBtn} ${copied ? styles.actionBtnSuccess : ""}`}
            onClick={handleCopy}
            aria-label="Copy to clipboard"
          >
            {copied ? <Check size={14} /> : <Copy size={14} />}
            {copied ? "Copied!" : "Copy"}
          </button>
          <button
            className={styles.actionBtn}
            onClick={onDownload}
            aria-label="Download config.toml"
          >
            <Download size={14} />
          </button>
        </div>
      </div>
      <div className={styles.codeWrapper}>
        <div className={styles.lineNumbers} aria-hidden="true">
          {Array.from({ length: lineCount }, (_, i) => (
            <span key={i}>{i + 1}</span>
          ))}
        </div>
        <pre className={styles.code}>
          <code dangerouslySetInnerHTML={{ __html: highlighted }} />
        </pre>
      </div>
    </div>
  );
}
