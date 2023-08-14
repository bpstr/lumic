import { defineConfig } from 'vitepress'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  title: "Lumic Control Center",
  description: "Server Management application for ubuntu VPS",
  themeConfig: {
    // https://vitepress.dev/reference/default-theme-config
    nav: [
      { text: 'Home', link: '/' },
      { text: 'Install guide', link: '/install-guide' },
      { text: 'API Reference', link: '/api/getting-started' }
    ],

    sidebar: [
      {
        text: 'User Guide',
        items: [
          { text: 'Getting Started', link: '/guide/getting-started' },
          { text: 'Admin Dashboard', link: '/guide/admin-dashboard' },
          { text: 'Creating Servers', link: '/guide/creating-servers' },
          { text: 'Server Details', link: '/guide/server-details' }
        ]
      },
      {
        text: 'API Reference',
        items: [
          { text: 'GET Status', link: '/api/status' },
          { text: 'Runtime API Examples', link: '/api-examples' }
        ]
      }
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/bpstr/lumic' }
    ],
    footer: {
      message: 'Released under the MIT License.',
      copyright: 'Copyright © 2023 bpstr'
    }

  }
})
