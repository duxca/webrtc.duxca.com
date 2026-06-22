# Cloudflare Terraform

This directory manages the Cloudflare DNS record for `webrtc.duxca.com`.

## Usage

```bash
cd terraform_cf
CLOUDFLARE_API_TOKEN=... terraform plan -var-file=prod.tfvars
CLOUDFLARE_API_TOKEN=... terraform apply -var-file=prod.tfvars
```

