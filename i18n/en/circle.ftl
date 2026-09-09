app-title = Circle
# Also the desktop entry's Comment and the AppStream <summary>, via build.rs.
app-comment = Find and edit the people in your address book
# Semicolon-separated, matching the desktop-entry convention. build.rs splits
# them into one <keyword> element each.
app-keywords = contact;contacts;address;addressbook;vcard;person;people;phone;email;

## Menus and shell

file = File
edit = Edit
view = View
about = About
settings = Settings
repository = Repository
support = Support

all-contacts = All contacts
address-books = Address books
contacts-count = { $count } { $count ->
        [one] contact
       *[other] contacts
    }

## The list

search-contacts = Search contacts…
no-contacts = No contacts yet.
no-contacts-description = Add a CardDAV account to sync your address book, or create a contact.
no-search-results = No contacts match “{ $query }”.
no-selection = Select a contact to see their details.

## Detail pane

email = Email
phone = Phone
address = Address
organisation = Organisation
title = Job title
note = Note
birthday = Birthday
nickname = Nickname
website = Website
categories = Categories
copy = Copy
copy-email = Copy email
copy-phone = Copy phone
copied = Copied to the clipboard
send-mail = Write an email
call = Call

# The section listing properties Circle parses but does not let you edit.
other-fields = Also on this card
other-fields-description =
    Circle keeps these exactly as they arrived and does not change them when
    you save.
has-photo = Photo
in-book = Address book

## Editing

new-contact = New contact
edit-contact = Edit contact
delete-contact = Delete contact
save = Save
cancel = Cancel
delete = Delete
add = Add
remove = Remove
refresh = Refresh

name-given = First name
name-family = Last name
name-additional = Middle name
name-prefix = Title
name-suffix = Suffix
display-name = Display name
display-name-description = Leave empty to use the first and last name.

label-home = Home
label-work = Work
label-mobile = Mobile
label-other = Other

set-preferred = Preferred
address-street = Street
address-extended = Apartment, suite
address-locality = City
address-region = Region
address-postcode = Postcode
address-country = Country

birthday-format = Birthday, as YYYY-MM-DD
categories-hint = Separate categories with commas. A comma inside one category is written \\,

add-email = Add an email address
add-phone = Add a phone number
add-address = Add an address
add-url = Add a website
add-nickname = Add a nickname

photo = Photo
set-photo = Set photo…
undo = Undo
photo-current = This card has a photo.
photo-none = No photo.
photo-pending = { $name } will be set on save.
photo-removing = The photo is removed on save.
error-photo = The photo could not be changed: { $why }

groups = Groups
new-group = New group
group-name = Group name
create = Create
confirm-delete-group-body =
    Only the group itself is deleted — the people in it stay in your address
    book. If the book is synced, the deletion is pushed on the next sync.
in-groups = In groups

confirm-delete-title = Delete { $name }?
confirm-delete-body =
    The contact file is removed from this address book. If the book is synced,
    the deletion is pushed to the server on the next sync.

## Import and export

import = Import contacts…
export = Export…
import-empty = Nothing in { $path } could be read as a contact.
import-done = Imported { $added } new, updated { $updated }.
export-done = Saved { $path }.
error-remote-file = That location is not a local file.

## CSV import

import-csv = Import CSV…
csv-skip = Skip
csv-uid = Unique ID
csv-columns = Map { $count } rows
csv-empty = There is nothing to import in that file.
csv-import-done = Imported { $added }, updated { $updated }{ $skipped ->
        [0] {""}
       *[other] , skipped { $skipped } unnamed
    }.

## Settings

prefer-vcard4 = New contacts use vCard 4.0
prefer-vcard4-description =
    Off writes vCard 3.0, which every CardDAV server accepts. Existing
    contacts always keep their own version.
sort-by-given-name = Sort by first name
sort-by-given-name-description = Off sorts by last name, the way a phone book reads.
default-book = New contacts go to
show-book = Show this address book

## Errors

error-load-contacts = Could not load contacts.
error-save = Could not save { $name }: { $why }
error-delete = Could not delete { $name }: { $why }
error-no-writable-book =
    There is no address book that can be written to. Add a CardDAV account, or
    create a directory under your contacts folder.
read-only-book = { $name } is read-only.

## Accounts — mirrors Slate's page: same store underneath, same four questions.

accounts = Accounts
sync = Sync
add-account = Add account…
account-name = Name
server-url = Server address
username = Username
password = Password
app-password-hint = Many providers require an app-specific password rather than your normal one.
no-accounts-description =
    Add a CardDAV account to sync your contacts with a server. Accounts are
    shared with Slate — an account added there is already here.
sync-now = Sync now
syncing = Syncing…
sync-interval = Background sync
sync-interval-description = How often to sync accounts on their own. Sync now always works.
sync-off = Only when I press Sync
sync-minutes = Every { $minutes ->
        [60] hour
       *[other] { $minutes } minutes
    }
error-no-account-store = Account storage is unavailable, so accounts cannot be saved.
error-url-scheme = The server address must start with https://
error-url-insecure = Refusing to send your password over an unencrypted connection. Use https://

## Selection and bulk actions

select = Select
select-all = Select all
selected-count = { $count ->
        [one] { $count } selected
       *[other] { $count } selected
    }
add-to-group = Add to group…
add-to-group-title = Add { $count ->
        [one] one contact
       *[other] { $count } contacts
    } to a group
add-to-group-body =
    They join the group with this name — an existing name adds to that group,
    a new one creates it in the sidebar.
added-to-group = Added { $count } to { $name }.
confirm-delete-many-title = Delete { $count } contacts?
deleted-one = Deleted { $name }.
deleted-many = Deleted { $count } contacts.

## Linking and duplicates

link = Link
unlink = Unlink
linked-cards = Linked cards
link-needs-two = Pick at least two contacts to link.
linked-count = Linked { $count } cards into one person.
unlinked = { $name } is its own contact again.
find-duplicates = Find duplicates…
no-duplicates = No possible duplicates found.
review-duplicates = Possible duplicates
review-remaining = { $count ->
        [one] One pair to review
       *[other] { $count } pairs to review
    }
review-explains-linking =
    Linking keeps both cards exactly as they are and shows them as one person.
    Each card goes on syncing to its own account.
match-email = Both have { $value }
match-phone = Both have { $value }
match-name = The names look alike — a guess, not evidence
not-the-same = Not the same
close = Close
editing-card = Editing the card in { $book }
share-contact = Share…
share-hint = Point a phone camera at the code to add this contact.
share-too-big = This contact has too much in it to fit in a QR code.
send = Send
sms-to = Text { $number }
sms-via = Sent through { $device }.
sms-body = Message
sms-sent = Message sent.
sms-failed = The message could not be sent: { $why }
sms = Text
open-link = Open in a browser
back-to-list = Back to the list

## Keeping in touch

keep-in-touch = Keep in touch
last-contact = Last in touch
last-contacted = { $when }
last-contacted-never = Never
log-interaction = Log a contact
overdue = Overdue
notes = Notes
note-placeholder = Add a note…
notes-are-local =
    Notes stay on this computer. They are not written to the contact's card,
    so nothing here is sent to a server or seen by anyone you share an
    address book with.

cadence-none = No reminder
cadence-weekly = Weekly
cadence-fortnightly = Fortnightly
cadence-monthly = Monthly
cadence-quarterly = Every three months
cadence-twice-yearly = Twice a year
cadence-yearly = Yearly

when-today = Today
when-yesterday = Yesterday
when-tomorrow = Tomorrow
when-days-ago = { $days ->
        [one] { $days } day ago
       *[other] { $days } days ago
    }
when-in-days = { $days ->
        [one] In { $days } day
       *[other] In { $days } days
    }
cadence = Remind me to be in touch
related = Related
relationships = Related people
attachment-too-big = That file is { $size }, over the { $limit } limit for attachments.
attachments = Attachments
attach-file = Attach a file…
open = Open
attachments-are-local =
    Attachments stay on this computer, beside your address books. They are not
    written to the contact's card and never reach a server.
attachment-added = Attached { $name }.
attachment-missing = { $name } is no longer on disk.
