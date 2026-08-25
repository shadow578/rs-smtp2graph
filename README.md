# SMTP2Graph - SMTP to Microsoft Graph API proxy

SMTP2Graph is a simple tool that acts as a proxy between legacy SMTP clients and the Microsoft Graph API.
it allows you to send mails via Microsoft Graph API using any SMTP client.

## Why?

as microsoft is deprecating basic authentication for SMTP, it becomes more difficult to send emails from legacy applications.
while some software vendors are updating their applications to support either SMTP via OAuth2 or Microsoft Graph API, many legacy applications will never receive such an update. 
add to that the many crappy software vendors that try to add support for OAuth2 but completely fail to do it correctly (yes, and i had to deal with it), and sooner or later you'll have to find an alternative solution for sending mail.   
   
of course, microsoft provides a solution via [SMTP relay](https://learn.microsoft.com/en-us/exchange/mail-flow-best-practices/how-to-set-up-a-multifunction-device-or-application-to-send-email-using-microsoft-365-or-office-365), but frankly, that is just a shit solution.
like sure, let's just let *anyone* on your ip send mail in your name, i'm sure nobody would ever abuse that.  
   
  
so, i decided to write a simple proxy that accepts mail via SMTP, then relays them to Microsoft Graph API.
simple as that.


## How it works

SMTP2Graph consists of two main components:
- an (custom\*) SMTP server that accepts mail from legacy SMTP clients. To offer flexibility, both TLS and non-TLS, as well as optional authentication are supported.
- a Microsoft Graph API client that relays mails via the [sendMail](https://learn.microsoft.com/en-us/graph/api/user-sendmail) function via client credentials.

the rest of the code is just glue code to translate from SMTP to Graph API, a configuration cli, and some boilerplate to let you run this as a windows service.


\*: i wrote a custom SMTP server instead of using something like mailin_embedded since i needed extra flexibility. E.g. auth without TLS.


## Setup

first, install this service.
on windows, you can run `irm https://raw.githubusercontent.com/shadow578/rs-smtp2graph/refs/heads/main/Install-SMTP2GraphService.ps1 | iex` in an elevated powershell prompt to automatically download and install the service.
on linux, you'll probalby figure it out ;).

once installed, configure the service using the config cli: `smtp2graph config <...>`


### Configure SMTP

by default, the service will listen on port 25 of the loopback interface (`127.0.0.1:25`).
unless your smtp client is running on the same machine, you'll need to change the listen address such that all interfaces are listened to (`0.0.0.0:25`).
to do so, run `smtp2graph config smtp update --address "0.0.0.0:25"`.
if you want to use a differnt port, the same command can be used, simply change the port number in the address.


#### Configure TLS

if you want to use TLS, you'll need to provide a certificate and private key in PEM format.
use this command to generate a self-signed certificate and private key:
```
openssl req -newkey rsa:2048 -x509 -sha256 -days 3650 -nodes -subj "/CN=localhost" -addext "subjectAltName=DNS:localhost,IP:127.0.0.1" -out my_cert.crt -keyout my_cert.key
```

then, configure the service to use the certificate and private key: `smtp2graph config tls setup --certificate my_cert.crt --private-key my_cert.key`.
to update the certificate and private key, simply run the same command again with the new files.


### Configure Microsoft Entra App

create a new app registration in Microsoft Entra ID, give it a name ("smtp2graph" or whatever), and note the application (client) and directory (tenant) id.
now, create a new client secret and note the value.
to configure the service, use `smtp2graph config graph update --tenant-id <tenant_id> --client-id <client_id> --client-secret <client_secret>`.
if you need to update the client secret, run `smtp2graph config graph update --client-secret <new_client_secret>`. the tenant and client id will remain the same.
   
   
for the simplest setup, simply grant the application the `Mail.Send` application permission, and then grant admin consent for the permission.
this will allow the application to send mail as any user in the tenant.
for more fine-grained control, use [Exchange Online's RBAC for Applications](https://learn.microsoft.com/en-us/exchange/permissions-exo/application-rbac) to restrict the application to only send mail as specific users.


#### Configuring RBAC for Applications

to use RBAC for Applications, do **not** grant the `Mail.Send` application permission to the application.
instead, run these commands to create a new service principal, assign it a management scope, and assign the `Mail.Send` role:

```powershell
Connect-ExchangeOnline

# create a new service principal for the application
# you need to supply the application id and object id as they are listed under enterprise apps, not the app registration
New-ServicePrincipal -AppId "<app_id>" -ObjectId "<object_id>" -DisplayName "smtp2graph"
$sp = Get-ServicePrincipal -Identity "smtp2graph"

# create a new management scope.
# this command will simply include all mailboxes, but you can also restrict it to specific mailboxes or groups if you want.
New-ManagementScope -Name "smtp2graph-scope" -RecipientRestrictionFilter { RecipientTypeDetails -eq "UserMailbox" -or RecipientTypeDetails -eq "SharedMailbox" }

# now, we assign smtp2graph the Mail.Send role for the management scope we just created
New-ManagementRoleAssignment -Name "smtp2graph-role" -Role "Application SMTP.SendAsApp" -App $sp -CustomResourceScope "smtp2graph-scope"

# you can verify if a mailbox is in-scope with this command.
# it should show InScope = true.
Test-ServicePrincipalAuthorization -Identity $sp -Resource "you@example.com"
```

### Configure Authentication

by default, the procy does not require authentication, and will accept mail from any client.
to change this, you can add users via the config cli: `smtp2graph config user add <username> [password]`.
if you don't provide a password, a random one will be generated and printed to the console.

the username used during authentication *must* match the sender address of the mail being sent, and must be a valid mailbox in your tenant.


## License

this project is licensed under the GNU General Public License v3.0 - see the [LICENSE](LICENSE) file for details.   
   

that means that you are free to use, modify and redistribute this software, but you must also distribute your modifications under the same license.
this includes using this software in a commercial context, e.g. instaling it on a customers server.
while this license doesn't *strictly* require you to disclose the use of this software to your customers, it'd be really nice if you did.  

no liability is assumed for any damage caused by this software, and it is provided "as-is" without any warranty.

