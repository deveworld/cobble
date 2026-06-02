import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Cobble",
  description:
    "A modern, Python-like language for creating Minecraft Java Edition data packs."
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
