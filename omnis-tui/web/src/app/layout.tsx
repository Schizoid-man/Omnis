import type { Metadata } from 'next'
import './globals.css'

export const metadata: Metadata = {
  title: 'Omnis Desktop',
  description: 'Omnis desktop control panel',
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang="en" className="dark">
      <body className="bg-background text-foreground antialiased">{children}</body>
    </html>
  )
}
