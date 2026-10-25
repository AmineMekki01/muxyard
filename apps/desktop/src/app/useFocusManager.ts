/** Shell focus ownership and collapsible-region state. */

import { useRef, useState } from "react";

import type { FocusRegion } from "./keymap";

export function useFocusManager() {
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [inspectorOpen, setInspectorOpen] = useState(true);
  const [activeRegion, setActiveRegion] = useState<FocusRegion | null>(null);
  const sidebarRef = useRef<HTMLElement>(null);
  const mainRef = useRef<HTMLElement>(null);
  const inspectorRef = useRef<HTMLElement>(null);

  const focusRegion = (region: FocusRegion) => {
    const refs = {
      sidebar: sidebarRef,
      main: mainRef,
      inspector: inspectorRef,
    } as const;
    const needsSidebar = region === "sidebar" && !sidebarOpen;
    const needsInspector = region === "inspector" && !inspectorOpen;
    if (needsSidebar) setSidebarOpen(true);
    if (needsInspector) setInspectorOpen(true);
    const focus = () => {
      const element = refs[region].current;
      if (!element) return;
      element.focus();
      setActiveRegion(region);
    };
    if (needsSidebar || needsInspector) {
      setTimeout(focus, 0);
    } else {
      focus();
    }
  };

  const toggleSidebar = () => {
    if (sidebarOpen && activeRegion === "sidebar") focusRegion("main");
    setSidebarOpen(!sidebarOpen);
  };

  const toggleInspector = () => {
    if (inspectorOpen && activeRegion === "inspector") focusRegion("main");
    setInspectorOpen(!inspectorOpen);
  };

  return {
    activeRegion,
    setActiveRegion,
    focusRegion,
    sidebarOpen,
    inspectorOpen,
    toggleSidebar,
    toggleInspector,
    sidebarRef,
    mainRef,
    inspectorRef,
  };
}
