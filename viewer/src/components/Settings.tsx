import { useEffect } from "react";
import { useHelp } from "../help";
import { Lang, NAMES, SUPPORTED, useI18n } from "../i18n";

/**
 * Settings dialog, matching the one webflash and the web flasher family use:
 * a dimmed full-screen overlay, a centred card with a gear-titled header and
 * a close button, the language row first in the body, and the action button
 * in a footer. Language applies on change there too, so there is nothing to
 * commit and the footer carries a single Close.
 *
 * Escape and a click on the backdrop also dismiss it. The siblings only offer
 * the buttons, but neither changes how the dialog looks and both are what a
 * dialog is expected to do.
 */
export default function Settings({ onClose }: { onClose: () => void }) {
  const { lang, setLang, t } = useI18n();
  const { on: helpOn, setOn: setHelpOn } = useHelp();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="overlay"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="dialog" role="dialog" aria-modal="true" aria-label={t("settings_title")}>
        <div className="dialog-head">
          <span className="dialog-title">
            <GearIcon /> {t("settings_title")}
          </span>
          <button
            className="dialog-x"
            onClick={onClose}
            title={t("title_close")}
            aria-label={t("title_close")}
          >
            ✕
          </button>
        </div>
        <div className="dialog-body">
          <div className="setting-row">
            <label className="setting-label" htmlFor="lang-select">
              {t("settings_lang")}
            </label>
            <select
              id="lang-select"
              className="select"
              value={lang}
              onChange={(e) => setLang(e.target.value as Lang)}
            >
              {SUPPORTED.map((l) => (
                <option key={l} value={l}>
                  {NAMES[l]}
                </option>
              ))}
            </select>
          </div>
          <label className="setting-check">
            <input
              type="checkbox"
              checked={helpOn}
              onChange={(e) => setHelpOn(e.target.checked)}
            />
            <span>{t("setting_help_label")}</span>
          </label>
        </div>
        <div className="dialog-foot">
          <button className="btn btn-sm btn-outline" onClick={onClose}>
            {t("btn_close")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Gear glyph, drawn rather than pulled from an icon font. */
export function GearIcon() {
  return (
    <svg
      className="icon"
      viewBox="0 0 16 16"
      width="13"
      height="13"
      fill="currentColor"
      aria-hidden="true"
      focusable="false"
    >
      <path d="M9.05.435c-.58-.58-1.52-.58-2.1 0L6.47 1.01a1.49 1.49 0 0 1-1.32.42l-.8-.13a1.49 1.49 0 0 0-1.7 1.22l-.13.8a1.49 1.49 0 0 1-.82 1.09l-.72.36a1.49 1.49 0 0 0-.65 2l.36.72c.22.44.22.96 0 1.4l-.36.72a1.49 1.49 0 0 0 .65 2l.72.36c.4.2.7.6.82 1.09l.13.8a1.49 1.49 0 0 0 1.7 1.22l.8-.13c.48-.08.98.07 1.32.42l.58.57c.58.58 1.52.58 2.1 0l.58-.57c.34-.35.84-.5 1.32-.42l.8.13a1.49 1.49 0 0 0 1.7-1.22l.13-.8c.12-.49.42-.89.82-1.09l.72-.36a1.49 1.49 0 0 0 .65-2l-.36-.72a1.6 1.6 0 0 1 0-1.4l.36-.72a1.49 1.49 0 0 0-.65-2l-.72-.36a1.49 1.49 0 0 1-.82-1.09l-.13-.8a1.49 1.49 0 0 0-1.7-1.22l-.8.13a1.49 1.49 0 0 1-1.32-.42zM8 11a3 3 0 1 1 0-6 3 3 0 0 1 0 6" />
    </svg>
  );
}
