# DNS Configuration for volt.setique.com

## Step 1: Add Custom Domain in Netlify

1. Go to https://app.netlify.com
2. Select project: **volt-setique**
3. Navigate to **Domain settings**
4. Click **Add custom domain**
5. Enter: `volt.setique.com`
6. Click **Verify** and **Add domain**

## Step 2: Configure DNS at Your Registrar

### If using Cloudflare:
```
Type: CNAME
Name: volt
Content: volt-setique.netlify.app
TTL: Auto
Proxy status: DNS only (gray cloud)
```

### If using GoDaddy:
```
Type: CNAME
Host: volt
Points to: volt-setique.netlify.app
TTL: 1 hour
```

### If using Namecheap:
```
Type: CNAME
Host: volt
Value: volt-setique.netlify.app
TTL: Automatic
```

### If using AWS Route53:
```
Type: CNAME
Name: volt
Value: volt-setique.netlify.app
TTL: 300
```

## Step 3: Verify Configuration

After updating DNS records (can take 5-60 minutes):

1. Run: `nslookup volt.setique.com`
2. You should see: `volt-setique.netlify.app`
3. Visit: https://volt.setique.com

## Current Deployment Status

- **Netlify Project**: volt-setique
- **Default URL**: https://volt-setique.netlify.app
- **Custom Domain**: volt.setique.com (pending DNS configuration)

## Troubleshooting

If the domain doesn't resolve after 60 minutes:
1. Check DNS propagation: https://dnschecker.org/
2. Verify CNAME record is correct
3. Ensure no typos in domain name
4. Check Netlify domain settings for errors

## Alternative: Use Existing Subdomain

If volt.setique.com is not available, you can use:
- **labs.setique.com** → Redirect to volt-setique
- **setique.com/volt** → Add routing rule

## SSL Certificate

Netlify automatically provisions SSL certificates for custom domains. No additional configuration needed.