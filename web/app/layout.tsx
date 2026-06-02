import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Cobble Demo",
  description:
    "Compile Python-like Cobble code into Minecraft data pack mcfunction output in the browser."
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
