# beancount-gocardless-importer

Beancount importer using gocardless.com APIs

Beancount: https://beancount.github.io/docs/index.html

Status: Work in progress

## Setup

1. Install the importer:

   ```shell
   cargo install --git https://github.com/doriath/beancount-gocardless-importer.git
   ```

2. Get API keys: https://gocardless.com/bank-account-data/

3. Sign in:

   ```shell
   beancount-gocardless-importer sign-in <secret-id> <secret-key>
   ```

4. Find an insitution you want to connect:

   ```shell
   beancount-gocardless-importer list-institutions --country=<country code>
   ```

4. Connect to institution:

   ```shell
   beancount-gocardless-importer create-requisition <instituion-id>
   ```

4. Verify the connection was successful (the list of accounts should be present):

   ```shell
   beancount-gocardless-importer list-requisitions
   ```

5. List transactions from the account:

   ```shell
   beancount-gocardless-importer list-transactions <account-id>
   ```
