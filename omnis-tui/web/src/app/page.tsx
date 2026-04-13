import { OmnisDashboard } from '@/components/omnis-dashboard'

export default function HomePage() {
  return (
    <main className="h-svh w-full overflow-hidden bg-background p-2 md:p-3">
      <div className="flex h-full w-full flex-col gap-2 overflow-hidden md:gap-3">
        <OmnisDashboard />
      </div>
    </main>
  )
}
