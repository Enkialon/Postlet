/** 平台能力：打开外链等，不复用邮件业务 Port */
export interface PlatformPort {
  openExternalUrl(url: string): Promise<void>;
}
