"use client";

import { useEffect, useRef } from "react";
import styles from "./landing-hero-interactive.module.css";

type PointerPoint = { x: number; y: number };

export function LandingHeroInteractive() {
  const sceneRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const scene = sceneRef.current;
    if (!scene) return;

    const prefersReducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const supportsPointer = window.matchMedia("(hover: hover) and (pointer: fine)").matches;
    const hero = document.querySelector<HTMLElement>("[data-landing-hero]");
    if (!hero || !supportsPointer || prefersReducedMotion) return;

    let pointerFrame = 0;
    let currentPoint: PointerPoint | null = null;

    const setSceneActive = (value: boolean) => {
      if (!scene || !hero) return;
      scene.classList.toggle(styles.active, value);
      hero.dataset.hermesActive = value ? "true" : "false";
    };

    const updateScene = () => {
      pointerFrame = 0;
      const heroRect = hero.getBoundingClientRect();
      if (!currentPoint) {
        setSceneActive(false);
        return;
      }

      const withinX = currentPoint.x >= heroRect.left && currentPoint.x <= heroRect.right;
      const withinY = currentPoint.y >= heroRect.top && currentPoint.y <= heroRect.bottom;
      const inside = withinX && withinY;

      if (!inside) {
        setSceneActive(false);
        return;
      }

      const offsetX = ((currentPoint.x - heroRect.left) / heroRect.width) * 100;
      const offsetY = ((currentPoint.y - heroRect.top) / heroRect.height) * 100;
      scene.style.setProperty("--pointer-x", `${offsetX.toFixed(2)}%`);
      scene.style.setProperty("--pointer-y", `${offsetY.toFixed(2)}%`);
      setSceneActive(true);
    };

    const onPointerMove = (event: PointerEvent) => {
      currentPoint = { x: event.clientX, y: event.clientY };
      if (!pointerFrame) pointerFrame = window.requestAnimationFrame(updateScene);
    };

    const onPointerLeave = () => {
      currentPoint = null;
      setSceneActive(false);
    };

    hero.addEventListener("pointermove", onPointerMove);
    hero.addEventListener("pointerleave", onPointerLeave);

    return () => {
      hero.removeEventListener("pointermove", onPointerMove);
      hero.removeEventListener("pointerleave", onPointerLeave);
      if (pointerFrame) window.cancelAnimationFrame(pointerFrame);
      setSceneActive(false);
    };
  }, []);

  return (
    <div className={styles.interactiveScene} ref={sceneRef} aria-hidden="true">
      <picture>
        <source type="image/avif" media="(max-width: 767px)" srcSet="/hermes/hermes-hero-mobile.avif" />
        <source type="image/avif" media="(min-width: 768px)" srcSet="/hermes/hermes-hero-desktop.avif" />
        <source type="image/webp" media="(max-width: 767px)" srcSet="/hermes/hermes-hero-mobile.webp" />
        <img
          className={styles.heroBase}
          src="/hermes/hermes-hero-desktop.webp"
          alt=""
          width={1672}
          height={941}
        />
      </picture>

      <span className={styles.heroGlow} />

      <picture>
        <source type="image/avif" media="(max-width: 767px)" srcSet="/hermes/hermes-foreground-mobile.avif" />
        <source type="image/avif" media="(min-width: 768px)" srcSet="/hermes/hermes-foreground-desktop.avif" />
        <source type="image/webp" media="(max-width: 767px)" srcSet="/hermes/hermes-foreground-mobile.webp" />
        <img
          className={styles.heroForeground}
          src="/hermes/hermes-foreground-desktop.webp"
          alt=""
          width={1672}
          height={941}
        />
      </picture>

      <svg className={styles.heroLightning} viewBox="0 0 1672 941" preserveAspectRatio="xMidYMid slice" aria-hidden="true">
        <g className={`${styles.heroStrike} ${styles.heroStrikeOne}`}>
          <path d="M1017 24 1023 57 1012 83 1030 111 1018 139 1036 171 1024 202 1047 231 1036 267 1052 298 1042 332 1059 365" />
          <path d="M1017 24 1023 57 1012 83 1030 111 1018 139 1036 171 1024 202 1047 231 1036 267 1052 298 1042 332 1059 365" />
          <path d="M1010 111 1031 99 1042 78 M1047 231 1012 242 995 261" />
        </g>
        <g className={`${styles.heroStrike} ${styles.heroStrikeTwo}`}>
          <path d="M1398 62 1379 91 1390 113 1364 140 1385 161 1357 190 1380 214 1351 245 1375 272" />
          <path d="M1398 62 1379 91 1390 113 1364 140 1385 161 1357 190 1380 214 1351 245 1375 272" />
          <path d="M1385 161 1414 151 1431 130 M1380 214 1408 228 1426 251" />
        </g>
        <g className={`${styles.heroStrike} ${styles.heroStrikeThree}`}>
          <path d="M1278 215 1290 244 1275 267 1302 291 1285 318 1312 346 1296 372 1324 401" />
          <path d="M1278 215 1290 244 1275 267 1302 291 1285 318 1312 346 1296 372 1324 401" />
          <path d="M1302 291 1334 280 1352 259 M1285 318 1254 335 1239 360" />
        </g>
        <g className={`${styles.heroStrike} ${styles.heroStrikeFour}`}>
          <path d="M1169 365 1184 397 1165 422 1193 450 1175 482 1207 512 1186 543 1216 577 1196 610 1225 646" />
          <path d="M1169 365 1184 397 1165 422 1193 450 1175 482 1207 512 1186 543 1216 577 1196 610 1225 646" />
          <path d="M1193 450 1222 438 1241 416 M1207 512 1240 526 1260 549" />
        </g>
        <g className={`${styles.heroStrike} ${styles.heroStrikeFive}`}>
          <path d="M1339 522 1355 552 1338 578 1362 604 1345 631 1374 658 1357 688 1387 716 1369 747 1400 781" />
          <path d="M1339 522 1355 552 1338 578 1362 604 1345 631 1374 658 1357 688 1387 716 1369 747 1400 781" />
          <path d="M1362 604 1392 593 1411 572 M1374 658 1342 673 1324 697" />
        </g>
      </svg>
    </div>
  );
}
