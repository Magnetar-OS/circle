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
send-email = Send an email
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
categories-hint = Separate categories with commas.

add-email = Add an email address
add-phone = Add a phone number
add-address = Add an address
add-url = Add a website
add-nickname = Add a nickname

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

## Settings

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
