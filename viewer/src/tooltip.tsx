import { ReactNode, useCallback, useState } from "react";

interface TipState {
  x: number;
  y: number;
  content: ReactNode;
}

export function useTooltip() {
  const [tip, setTip] = useState<TipState | null>(null);

  const show = useCallback((e: { clientX: number; clientY: number }, content: ReactNode) => {
    setTip({ x: e.clientX, y: e.clientY, content });
  }, []);
  const hide = useCallback(() => setTip(null), []);

  const node = tip ? (
    <div
      className="tooltip"
      style={{
        left: Math.min(tip.x + 14, window.innerWidth - 280),
        top: Math.min(tip.y + 14, window.innerHeight - 160),
      }}
    >
      {tip.content}
    </div>
  ) : null;

  return { node, show, hide };
}
