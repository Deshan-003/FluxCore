import type { ProcessMetrics } from '../types/monitoring'

// Pure search/sort rules for the Processes page, kept separate so they can be
// unit-tested: case-insensitive name match or PID substring match, CPU sort
// pushes unavailable readings last, and the table is capped at 100 rows.
export function filterProcesses(
  processes: ProcessMetrics[],
  search: string,
  sortBy: 'cpu' | 'memory',
  limit = 100,
): ProcessMetrics[] {
  const query = search.toLowerCase()
  return processes
    .filter((process) =>
      process.name.toLowerCase().includes(query) || String(process.pid).includes(query))
    .sort((a, b) =>
      sortBy === 'cpu'
        ? (b.cpuPercent ?? -1) - (a.cpuPercent ?? -1)
        : b.memoryBytes - a.memoryBytes)
    .slice(0, limit)
}
