import type { Metadata } from "next";
import { Instrument_Sans, Newsreader } from "next/font/google";
import "./globals.css";

const instrumentSans = Instrument_Sans({
  subsets: ["latin"],
  variable: "--font-instrument",
  display: "swap"
});

const newsreader = Newsreader({
  subsets: ["latin"],
  variable: "--font-newsreader",
  display: "swap"
});

export const metadata: Metadata = {
  title: "Hermes Trader",
  description: "Private Solana execution intelligence for operators",
  icons: {
    icon: [
      {
        url: "/assets/hermes-favicon.png",
        type: "image/png",
        sizes: "128x128"
      }
    ],
    shortcut: "/assets/hermes-favicon.png",
    apple: [
      {
        url: "/assets/hermes-logo.png",
        type: "image/png",
        sizes: "512x512"
      }
    ]
  }
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body className={`${instrumentSans.variable} ${newsreader.variable}`}>{children}</body>
    </html>
  );
}
