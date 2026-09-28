import { useLayoutEffect, useRef, useState } from "react";

/** A ref and the live pixel size of the element it is attached to. */
export function useElementSize<TElement extends HTMLElement>() {
  const ref = useRef<TElement | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });

  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) {
      return;
    }
    const observer = new ResizeObserver(([entry]) => {
      setSize({ width: entry.contentRect.width, height: entry.contentRect.height });
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return [ref, size] as const;
}
