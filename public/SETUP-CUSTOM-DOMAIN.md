# Configure Custom Domain for Volt

## Current Status
- **Netlify Site**: volt-setique (chic-gnome-d5382f)
- **Default URL**: https://chic-gnome-d5382f.netlify.app
- **Custom Domain**: volt.setique.com (needs configuration)

## Manual Steps to Add Custom Domain

### Step 1: Go to Netlify Dashboard
1. Visit: https://app.netlify.com
2. Click on **volt-setique** project
3. Navigate to **Site settings** > **Domain management**

### Step 2: Add Custom Domain
1. Click **Add custom domain**
2. Enter: `volt.setique.com`
3. Click **Verify**
4. Click **Add domain**

### Step 3: Configure DNS
After adding the domain in Netlify, you'll see DNS instructions. You need to add a CNAME record at your domain registrar:

**DNS Record to Add:**
```
Type: CNAME
Name: volt
Value: chic-gnome-d5382f.netlify.app
TTL: Auto (or 300 seconds)
```

### Step 4: Verify DNS
After updating DNS (can take 5-60 minutes):
1. Run: `nslookup volt.setique.com`
2. Should resolve to: `chic-gnome-d5382f.netlify.app`
3. Visit: https://volt.setique.com

## Alternative: Use Netlify DNS
If you use Netlify DNS for setique.com:
1. Go to **Domain settings** > **Netlify DNS**
2. Add CNAME record:
   - Name: `volt`
   - Target: `chic-gnome-d5382f.netlify.app`

## SSL Certificate
Netlify automatically provisions SSL certificates for custom domains. No additional configuration needed.

## Troubleshooting
- **Domain not resolving**: Check DNS propagation at https://dnschecker.org/
- **SSL pending**: Can take up to 24 hours
- **404 error**: Ensure DNS record is correct and propagated

## Once Complete
Your site will be live at: **https://volt.setique.com**