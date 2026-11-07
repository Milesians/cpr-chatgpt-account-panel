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
    viaAccountId?: string
  }) =>
    call<OAuthStartResponse>('POST', 'api/login/oauth/start', undefined, data),

  oauthComplete: (data: {
    accountId?: string
    email?: string
    kind?: string
    viaAccountId?: string
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
    viaAccountId?: string
    workspaceId?: string
    organizationId?: string
    replaceExisting?: boolean
    saveCredentials?: boolean
  }) =>
    call<ImportAccountResponse>('POST', 'api/login/import', undefined, data),
}
