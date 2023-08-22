# Getting Started with Lumic on Ubuntu 20.04

Welcome to the Lumic Server Manager setup guide! This guide will walk 
you through the initial steps to get your Lumic Server Manager up and 
running on a fresh instance of Ubuntu 20.04.

## Prerequisites
- A fresh Linode instance running Ubuntu 20.04.
- Root or sudo access to the server.
- Basic knowledge of terminal commands.

## Installation
Run the following command to install Lumic on Ubuntu 20.04.

```bash
bash <(curl -s https://raw.githubusercontent.com/bpstr/lumic/main/lumic.sh)
```

Read the detailed [installation guide](/install-guide.html) for more 
information.

## Accessing Lumic Server
1. Open your preferred web browser.
2. Enter the IP address of your Linode instance or the domain name if you've set one up.
3. You should now see the Lumic Server interface.

## Basic Configuration
1. Setting Up Your Profile:
    - Navigate to the profile section.
    - Update your username, email, and other relevant details.
2. Configuring Server Settings:
   - Go to the settings tab.
   - Adjust server parameters as needed, such as time zone, memory limits, and more.
   
## Security Tips
- Change Default Credentials: Always change default usernames and passwords to something unique and strong.
- Enable Two-Factor Authentication: If Lumic Server supports it, enable 2FA for an added layer of security.
- Regular Backups: Schedule regular backups of your Lumic Server data.

## Troubleshooting
- Logs: Check /var/log/installscript.log for any potential errors or warnings during the installation.
- Restart Services: If you encounter issues, try restarting the related service, e.g., sudo service nginx restart for web server issues.

## Conclusion
That's it! Remember to regularly check for updates and always 
follow the latest security practices. Enjoy using Lumic Server!




