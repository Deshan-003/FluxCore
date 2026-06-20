import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import ResourceIntelligencePage from './ResourceIntelligence'
import { useResourceStore } from '../stores/intelligence'
import { getResourceIntelligence, respondToResourceRecommendation } from '../services/intelligence'

vi.mock('../services/intelligence', () => ({
  applyResourcePolicy: vi.fn(async () => undefined),
  configureResourceIntelligence: vi.fn(async () => undefined),
  getResourceHistory: vi.fn(async () => ({ observations: [], comparisons: [], actions: [], decisions: [] })),
  getResourceIntelligence: vi.fn(),
  respondToResourceRecommendation: vi.fn(async () => undefined),
