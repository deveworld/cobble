import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Cobble",
  description:
    "A Python-like language that compiles to Minecraft Java Edition data packs."
};

export default function RootLayout({
  children
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
