import { invoke } from '@tauri-apps/api/core'
import type { Decision, ResourceHistory, ResourcePolicy, ResourceState } from '../types/intelligence'

export const getResourceIntelligence = () => invoke<ResourceState>('get_resource_intelligence')
