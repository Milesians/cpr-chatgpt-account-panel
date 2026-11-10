import type {
  AccountLoginStatus,
  PasteReport,
} from './helpers'
import { call } from '../api'

export interface LoginStatusOverview {
  oauthRedirect: string
  accounts: AccountLoginStatus[]
}

export interface SingleAccountLoginStatus {
  oauthRedirect: string
  account: AccountLoginStatus
}

export interface RunLoginResponse {
  ok: boolean
  account: AccountLoginStatus
  planType: string
  expiresAtMs: number
}

export interface OAuthStartResponse {
  authorizeUrl: string
  redirectUrl: string
  expiresAtMs: number
}

export interface OAuthCompleteResponse {
  ok: boolean
  accountId: string
  account?: AccountLoginStatus
  email?: string
  kind?: string
}

/** 导入时可选的 CPR 代理（宿主 host.proxies.list，不含地址和认证） */
export interface ImportProxy {
  id: string
  name: string
  enabled: boolean
}

export interface ImportAccountResponse {
  ok: boolean
  accountId: string
  email?: string
  kind?: string
  planType?: string
  hasPassword?: boolean
  hasTotp?: boolean
  account?: AccountLoginStatus
}

export const loginApi = {
  overview: () =>
    call<LoginStatusOverview>('GET', 'api/login/status'),

  proxies: () =>
    call<{ proxies: ImportProxy[] }>('GET', 'api/login/proxies'),

  accountStatus: (id: string) =>
    call<SingleAccountLoginStatus>('GET', 'api/login/status', { id }),

  saveCredentials: (data: {
    accountId: string
    email?: string
    password?: string
    totp?: string
    clearPassword?: boolean
    clearTotp?: boolean
  }) =>
    call<{ ok: boolean, account: AccountLoginStatus }>('POST', 'api/login/credentials', undefined, data),

  runLogin: (data: {
    accountId: string
    email?: string
    password?: string
    totp?: string
    saveCredentials?: boolean
    workspaceId?: string
    organizationId?: string
  }) =>
    call<RunLoginResponse>('POST', 'api/login/run', undefined, data),

  oauthStart: (data: {
    accountId?: string
    email?: string
    kind?: string
    proxyId?: string
  }) =>
    call<OAuthStartResponse>('POST', 'api/login/oauth/start', undefined, data),

  oauthComplete: (data: {
    accountId?: string
    email?: string
    kind?: string
    callbackUrl: string
  }) =>
    call<OAuthCompleteResponse>('POST', 'api/login/oauth/complete', undefined, data),

  paste: (text: string, apply = false) =>
    call<PasteReport>('POST', 'api/login/paste', undefined, { text, apply }),

  importAccount: (data: {
    email: string
    password?: string
    totp?: string
    kind: string
    proxyId?: string
    workspaceId?: string
    organizationId?: string
    replaceExisting?: boolean
    saveCredentials?: boolean
  }) =>
    call<ImportAccountResponse>('POST', 'api/login/import', undefined, data),
}
