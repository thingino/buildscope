/**
 * The way into a published fleet from the landing page. Without this the only
 * route in is a ?fleet= URL somebody already has to know about.
 *
 * It renders nothing until the release list actually loads and contains a
 * snapshot, so a deployment with none published -- or one that is simply
 * offline -- shows the drop target and nothing broken. That also keeps the
 * page honest as a general Buildroot tool: the menu appears because snapshots
 * exist, not because it is wired to any particular project.
 */
import { useEffect, useState } from "react";
import { fleetRepo, listReleases } from "../fleet";
import { useT } from "../i18n";

export default function FleetEntry() {
  const t = useT();
  const [tags, setTags] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let live = true;
    listReleases(fleetRepo())
      .then((ts) => {
        if (live) setTags(ts);
      })
      .catch(() => {
        /* No menu, rather than an error a reader here cannot act on. */
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, []);

  // Say it is looking rather than rendering nothing: an empty space reads as
  // "this page has no such feature", which is the wrong answer while the
  // question is still outstanding.
  if (loading) {
    return (
      <div className="fleet-entry">
        <div className="fleet-entry-title">{t("fleet_entry_title")}</div>
        <div className="fleet-entry-sub">{t("loading")}</div>
      </div>
    );
  }
  if (tags.length === 0) return null;

  // A full page load, not a state swap: the snapshot being read belongs in the
  // URL, so it can be shared and reloaded.
  const open = (tag: string) => {
    const u = new URL(location.href);
    u.searchParams.set("fleet", tag);
    u.hash = "";
    location.assign(u.toString());
  };

  return (
    <div className="fleet-entry">
      <div className="fleet-entry-title">{t("fleet_entry_title")}</div>
      <div className="fleet-entry-row">
        <button className="btn" data-help="help_open_latest" onClick={() => open(tags[0])}>
          {t("fleet_entry_latest")}
        </button>
        {tags.length > 1 && (
          <select
            className="select"
            defaultValue=""
            aria-label={t("fleet_entry_pick")}
            onChange={(e) => e.target.value && open(e.target.value)}
          >
            <option value="" disabled>
              {t("fleet_entry_pick")}
            </option>
            {tags.map((tag) => (
              <option key={tag} value={tag}>
                {tag}
              </option>
            ))}
          </select>
        )}
      </div>
      <div className="fleet-entry-sub">{t("fleet_entry_sub")}</div>
    </div>
  );
}
