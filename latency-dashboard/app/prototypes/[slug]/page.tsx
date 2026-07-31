import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { PrototypeDashboard } from "@/components/prototypes/prototype-dashboard";
import { PROTOTYPE_DIRECTIONS, directionForSlug } from "@/components/prototypes/prototype-data";

export function generateStaticParams() {
  return PROTOTYPE_DIRECTIONS.map(({ slug }) => ({ slug }));
}

export async function generateMetadata({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> {
  const { slug } = await params;
  const direction = directionForSlug(slug);
  return { title: direction ? `${direction.name} · Hermes Prototype` : "Hermes Prototype" };
}

export default async function PrototypePage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const direction = directionForSlug(slug);
  if (!direction) notFound();
  return <PrototypeDashboard direction={direction} />;
}
