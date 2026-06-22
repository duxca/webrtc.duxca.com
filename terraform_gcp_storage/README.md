# GCP Storage Terraform

This stack manages base Google Cloud resources for `webrtc.duxca.com`:

- required project APIs
- Artifact Registry repository
- Cloud Run runtime service account
- Secret Manager secret container for `WEBRTC_GITHUB_CLIENT_SECRET`
- IAM bindings for the runtime service account

The secret value itself is not managed by Terraform. Add version `1` before applying the app stack:

```bash
printf '%s' '<github client secret>' | gcloud secrets versions add WEBRTC_GITHUB_CLIENT_SECRET --data-file=-
```

## Usage

Create the Terraform state bucket manually and initialize `../terraform_bootstrap/bootstrap` first, then apply this stack:

```bash
cd terraform_gcp_storage
terraform init
terraform plan -var-file=prod.tfvars
terraform apply -var-file=prod.tfvars
```
