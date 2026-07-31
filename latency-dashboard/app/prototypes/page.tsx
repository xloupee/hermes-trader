import type { Metadata } from "next";
import { PrototypeGallery } from "@/components/prototypes/prototype-gallery";

export const metadata: Metadata = {
  title: "Dashboard Directions · Hermes Trader",
  description: "Eight working directions for the Hermes operator dashboard."
};

export default function PrototypesPage() {
  return <PrototypeGallery />;
}
